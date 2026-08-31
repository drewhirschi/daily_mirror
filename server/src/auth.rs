use std::io;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use libsql::{Builder, Database, params};
use rand_core::{OsRng, RngCore};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;
use uuid::Uuid;
use webauthn_rs::prelude::Passkey;

pub const SESSION_COOKIE: &str = "daily_mirror_session";
pub const SESSION_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);
pub const CEREMONY_TTL: Duration = Duration::from_secs(5 * 60);
const LOGIN_WINDOW: Duration = Duration::from_secs(15 * 60);
const LOGIN_BLOCK: Duration = Duration::from_secs(15 * 60);
const LOGIN_MAX_FAILURES: u32 = 8;

#[derive(Clone, Debug)]
pub struct AuthStore {
    inner: Arc<AuthInner>,
}

#[derive(Debug)]
struct AuthInner {
    location: AuthLocation,
    database: OnceCell<Database>,
}

#[derive(Debug)]
enum AuthLocation {
    Local(String),
    Remote { url: String, token: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub display_name: String,
}

impl User {
    pub fn uuid(&self) -> io::Result<Uuid> {
        Uuid::parse_str(&self.id).map_err(invalid_data)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PasskeySummary {
    pub credential_id: String,
    pub label: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

#[derive(Clone, Debug)]
pub struct StoredPasskey {
    pub credential_id: String,
    pub passkey: Passkey,
}

#[derive(Clone, Debug)]
pub struct SessionToken {
    pub token: String,
    pub max_age_seconds: u64,
}

#[derive(Clone, Debug)]
pub struct Ceremony {
    pub user_id: String,
    pub state_json: String,
}

impl AuthStore {
    pub fn local(path: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(AuthInner {
                location: AuthLocation::Local(path.into()),
                database: OnceCell::new(),
            }),
        }
    }

    pub fn from_env() -> io::Result<Self> {
        match std::env::var("DAILY_MIRROR_DATABASE_URL") {
            Ok(url) if url.starts_with("libsql://") || url.starts_with("https://") => {
                let token = std::env::var("DAILY_MIRROR_DATABASE_AUTH_TOKEN").map_err(|_| {
                    invalid_config("DAILY_MIRROR_DATABASE_AUTH_TOKEN is required for Turso")
                })?;
                Ok(Self {
                    inner: Arc::new(AuthInner {
                        location: AuthLocation::Remote { url, token },
                        database: OnceCell::new(),
                    }),
                })
            }
            Ok(path) if !path.is_empty() => Ok(Self::local(path)),
            _ => Ok(Self::local(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("data/daily-mirror.db")
                    .to_string_lossy()
                    .into_owned(),
            )),
        }
    }

    async fn database(&self) -> io::Result<&Database> {
        self.inner
            .database
            .get_or_try_init(|| async {
                let database = match &self.inner.location {
                    AuthLocation::Local(path) => {
                        if let Some(parent) = std::path::Path::new(path).parent() {
                            tokio::fs::create_dir_all(parent).await?;
                        }
                        Builder::new_local(path)
                            .build()
                            .await
                            .map_err(io::Error::other)?
                    }
                    AuthLocation::Remote { url, token } => {
                        Builder::new_remote(url.clone(), token.clone())
                            .build()
                            .await
                            .map_err(io::Error::other)?
                    }
                };
                let connection = database.connect().map_err(io::Error::other)?;
                connection
                    .execute_batch(
                        "CREATE TABLE IF NOT EXISTS users (
                            id TEXT PRIMARY KEY,
                            username TEXT NOT NULL COLLATE NOCASE UNIQUE,
                            display_name TEXT NOT NULL,
                            password_hash TEXT NOT NULL,
                            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                        );
                        CREATE TABLE IF NOT EXISTS auth_sessions (
                            token_hash TEXT PRIMARY KEY,
                            user_id TEXT NOT NULL,
                            expires_at INTEGER NOT NULL,
                            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                            FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
                        );
                        CREATE INDEX IF NOT EXISTS auth_sessions_user_expires
                            ON auth_sessions(user_id, expires_at);
                        CREATE TABLE IF NOT EXISTS passkeys (
                            credential_id TEXT PRIMARY KEY,
                            user_id TEXT NOT NULL,
                            label TEXT NOT NULL,
                            passkey_json TEXT NOT NULL,
                            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                            last_used_at TEXT,
                            FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
                        );
                        CREATE INDEX IF NOT EXISTS passkeys_user
                            ON passkeys(user_id, created_at);
                        CREATE TABLE IF NOT EXISTS auth_ceremonies (
                            token_hash TEXT PRIMARY KEY,
                            kind TEXT NOT NULL,
                            user_id TEXT NOT NULL,
                            state_json TEXT NOT NULL,
                            expires_at INTEGER NOT NULL,
                            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                            FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
                        );
                        CREATE TABLE IF NOT EXISTS auth_login_limits (
                            key_hash TEXT PRIMARY KEY,
                            failures INTEGER NOT NULL,
                            window_started_at INTEGER NOT NULL,
                            blocked_until INTEGER NOT NULL
                        );",
                    )
                    .await
                    .map_err(io::Error::other)?;
                Ok(database)
            })
            .await
    }

    pub async fn create_user(
        &self,
        username: &str,
        display_name: &str,
        password: &str,
    ) -> io::Result<User> {
        let username = normalize_username(username)?;
        let display_name = validate_display_name(display_name)?;
        validate_password(password)?;
        let password = password.to_owned();
        let password_hash = tokio::task::spawn_blocking(move || hash_password(&password))
            .await
            .map_err(io::Error::other)??;
        let user = User {
            id: Uuid::new_v4().to_string(),
            username,
            display_name,
        };
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        connection
            .execute(
                "INSERT INTO users (id, username, display_name, password_hash)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    user.id.clone(),
                    user.username.clone(),
                    user.display_name.clone(),
                    password_hash
                ],
            )
            .await
            .map_err(|error| io::Error::new(io::ErrorKind::AlreadyExists, error))?;
        Ok(user)
    }

    pub async fn verify_password(
        &self,
        username: &str,
        password: &str,
    ) -> io::Result<Option<User>> {
        let username = match normalize_username(username) {
            Ok(username) => username,
            Err(_) => return Ok(None),
        };
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        let mut rows = connection
            .query(
                "SELECT id, username, display_name, password_hash
                 FROM users WHERE username = ?1 COLLATE NOCASE",
                params![username],
            )
            .await
            .map_err(io::Error::other)?;
        let row = rows.next().await.map_err(io::Error::other)?;
        let (user, stored_hash) = match row {
            Some(row) => (
                Some(User {
                    id: row.get(0).map_err(io::Error::other)?,
                    username: row.get(1).map_err(io::Error::other)?,
                    display_name: row.get(2).map_err(io::Error::other)?,
                }),
                Some(row.get::<String>(3).map_err(io::Error::other)?),
            ),
            None => (None, None),
        };
        let password = password.to_owned();
        let verified = tokio::task::spawn_blocking(move || match stored_hash {
            Some(stored_hash) => verify_password_hash(&stored_hash, &password),
            None => {
                // Perform equivalent Argon2 work for unknown usernames so an
                // obvious timing gap does not reveal which accounts exist.
                let _ = hash_password(&password)?;
                Ok(false)
            }
        })
        .await
        .map_err(io::Error::other)??;
        Ok(user.filter(|_| verified))
    }

    pub async fn user_by_username(&self, username: &str) -> io::Result<Option<User>> {
        let username = match normalize_username(username) {
            Ok(username) => username,
            Err(_) => return Ok(None),
        };
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        let mut rows = connection
            .query(
                "SELECT id, username, display_name FROM users
                 WHERE username = ?1 COLLATE NOCASE",
                params![username],
            )
            .await
            .map_err(io::Error::other)?;
        row_to_user(rows.next().await.map_err(io::Error::other)?)
    }

    pub async fn login_retry_after(&self, scope: &str) -> io::Result<Option<u64>> {
        let now = unix_now()?;
        let key_hash = secret_hash(scope);
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        let mut rows = connection
            .query(
                "SELECT window_started_at, blocked_until FROM auth_login_limits
                 WHERE key_hash = ?1",
                params![key_hash.clone()],
            )
            .await
            .map_err(io::Error::other)?;
        let Some(row) = rows.next().await.map_err(io::Error::other)? else {
            return Ok(None);
        };
        let window_started_at: u64 = row.get(0).map_err(io::Error::other)?;
        let blocked_until: u64 = row.get(1).map_err(io::Error::other)?;
        if blocked_until > now {
            return Ok(Some(blocked_until - now));
        }
        if window_started_at.saturating_add(LOGIN_WINDOW.as_secs()) <= now {
            connection
                .execute(
                    "DELETE FROM auth_login_limits WHERE key_hash = ?1",
                    params![key_hash],
                )
                .await
                .map_err(io::Error::other)?;
        }
        Ok(None)
    }

    pub async fn record_login_failure(&self, scope: &str) -> io::Result<()> {
        let now = unix_now()?;
        let key_hash = secret_hash(scope);
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        connection
            .execute(
                "INSERT INTO auth_login_limits
                    (key_hash, failures, window_started_at, blocked_until)
                 VALUES (?1, 1, ?2, 0)
                 ON CONFLICT(key_hash) DO UPDATE SET
                    failures = CASE
                        WHEN auth_login_limits.window_started_at + ?3 <= ?2 THEN 1
                        ELSE auth_login_limits.failures + 1
                    END,
                    window_started_at = CASE
                        WHEN auth_login_limits.window_started_at + ?3 <= ?2 THEN ?2
                        ELSE auth_login_limits.window_started_at
                    END,
                    blocked_until = CASE
                        WHEN auth_login_limits.window_started_at + ?3 <= ?2 THEN 0
                        WHEN auth_login_limits.failures + 1 >= ?4 THEN ?2 + ?5
                        ELSE auth_login_limits.blocked_until
                    END",
                params![
                    key_hash,
                    now,
                    LOGIN_WINDOW.as_secs(),
                    LOGIN_MAX_FAILURES,
                    LOGIN_BLOCK.as_secs()
                ],
            )
            .await
            .map_err(io::Error::other)?;
        Ok(())
    }

    pub async fn clear_login_failures(&self, scope: &str) -> io::Result<()> {
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        connection
            .execute(
                "DELETE FROM auth_login_limits WHERE key_hash = ?1",
                params![secret_hash(scope)],
            )
            .await
            .map_err(io::Error::other)?;
        Ok(())
    }

    pub async fn user_by_id(&self, id: &str) -> io::Result<Option<User>> {
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        let mut rows = connection
            .query(
                "SELECT id, username, display_name FROM users WHERE id = ?1",
                params![id],
            )
            .await
            .map_err(io::Error::other)?;
        row_to_user(rows.next().await.map_err(io::Error::other)?)
    }

    pub async fn create_session(&self, user_id: &str) -> io::Result<SessionToken> {
        self.create_session_for(user_id, SESSION_TTL).await
    }

    async fn create_session_for(&self, user_id: &str, ttl: Duration) -> io::Result<SessionToken> {
        let token = random_token();
        let token_hash = secret_hash(&token);
        let expires_at = unix_now()?.saturating_add(ttl.as_secs());
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        connection
            .execute(
                "DELETE FROM auth_sessions WHERE expires_at <= ?1",
                params![unix_now()?],
            )
            .await
            .map_err(io::Error::other)?;
        connection
            .execute(
                "INSERT INTO auth_sessions (token_hash, user_id, expires_at)
                 VALUES (?1, ?2, ?3)",
                params![token_hash, user_id, expires_at],
            )
            .await
            .map_err(io::Error::other)?;
        Ok(SessionToken {
            token,
            max_age_seconds: ttl.as_secs(),
        })
    }

    pub async fn authenticate_session(&self, token: &str) -> io::Result<Option<User>> {
        if token.len() < 32 {
            return Ok(None);
        }
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        let mut rows = connection
            .query(
                "SELECT users.id, users.username, users.display_name
                 FROM auth_sessions
                 JOIN users ON users.id = auth_sessions.user_id
                 WHERE auth_sessions.token_hash = ?1 AND auth_sessions.expires_at > ?2",
                params![secret_hash(token), unix_now()?],
            )
            .await
            .map_err(io::Error::other)?;
        row_to_user(rows.next().await.map_err(io::Error::other)?)
    }

    pub async fn revoke_session(&self, token: &str) -> io::Result<()> {
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        connection
            .execute(
                "DELETE FROM auth_sessions WHERE token_hash = ?1",
                params![secret_hash(token)],
            )
            .await
            .map_err(io::Error::other)?;
        Ok(())
    }

    pub async fn store_ceremony(
        &self,
        kind: &str,
        user_id: &str,
        state_json: &str,
    ) -> io::Result<String> {
        let token = random_token();
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        connection
            .execute(
                "DELETE FROM auth_ceremonies WHERE expires_at <= ?1",
                params![unix_now()?],
            )
            .await
            .map_err(io::Error::other)?;
        connection
            .execute(
                "INSERT INTO auth_ceremonies (token_hash, kind, user_id, state_json, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    secret_hash(&token),
                    kind,
                    user_id,
                    state_json,
                    unix_now()?.saturating_add(CEREMONY_TTL.as_secs())
                ],
            )
            .await
            .map_err(io::Error::other)?;
        Ok(token)
    }

    pub async fn take_ceremony(&self, token: &str, kind: &str) -> io::Result<Option<Ceremony>> {
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        let mut rows = connection
            .query(
                "DELETE FROM auth_ceremonies
                 WHERE token_hash = ?1 AND kind = ?2 AND expires_at > ?3
                 RETURNING user_id, state_json",
                params![secret_hash(token), kind, unix_now()?],
            )
            .await
            .map_err(io::Error::other)?;
        let Some(row) = rows.next().await.map_err(io::Error::other)? else {
            return Ok(None);
        };
        Ok(Some(Ceremony {
            user_id: row.get(0).map_err(io::Error::other)?,
            state_json: row.get(1).map_err(io::Error::other)?,
        }))
    }

    pub async fn passkeys(&self, user_id: &str) -> io::Result<Vec<StoredPasskey>> {
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        let mut rows = connection
            .query(
                "SELECT credential_id, passkey_json FROM passkeys
                 WHERE user_id = ?1 ORDER BY created_at",
                params![user_id],
            )
            .await
            .map_err(io::Error::other)?;
        let mut passkeys = Vec::new();
        while let Some(row) = rows.next().await.map_err(io::Error::other)? {
            let credential_id: String = row.get(0).map_err(io::Error::other)?;
            let json: String = row.get(1).map_err(io::Error::other)?;
            passkeys.push(StoredPasskey {
                credential_id,
                passkey: serde_json::from_str(&json).map_err(invalid_data)?,
            });
        }
        Ok(passkeys)
    }

    pub async fn passkey_summaries(&self, user_id: &str) -> io::Result<Vec<PasskeySummary>> {
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        let mut rows = connection
            .query(
                "SELECT credential_id, label, created_at, last_used_at FROM passkeys
                 WHERE user_id = ?1 ORDER BY created_at",
                params![user_id],
            )
            .await
            .map_err(io::Error::other)?;
        let mut passkeys = Vec::new();
        while let Some(row) = rows.next().await.map_err(io::Error::other)? {
            passkeys.push(PasskeySummary {
                credential_id: row.get(0).map_err(io::Error::other)?,
                label: row.get(1).map_err(io::Error::other)?,
                created_at: row.get(2).map_err(io::Error::other)?,
                last_used_at: row.get(3).map_err(io::Error::other)?,
            });
        }
        Ok(passkeys)
    }

    pub async fn add_passkey(
        &self,
        user_id: &str,
        label: &str,
        passkey: &Passkey,
    ) -> io::Result<()> {
        let label = validate_passkey_label(label)?;
        let credential_id = URL_SAFE_NO_PAD.encode(passkey.cred_id());
        let json = serde_json::to_string(passkey).map_err(invalid_data)?;
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        connection
            .execute(
                "INSERT INTO passkeys (credential_id, user_id, label, passkey_json)
                 VALUES (?1, ?2, ?3, ?4)",
                params![credential_id, user_id, label, json],
            )
            .await
            .map_err(|error| io::Error::new(io::ErrorKind::AlreadyExists, error))?;
        Ok(())
    }

    pub async fn update_passkey(&self, user_id: &str, stored: &StoredPasskey) -> io::Result<()> {
        let json = serde_json::to_string(&stored.passkey).map_err(invalid_data)?;
        let connection = self.database().await?.connect().map_err(io::Error::other)?;
        let changed = connection
            .execute(
                "UPDATE passkeys SET passkey_json = ?3, last_used_at = CURRENT_TIMESTAMP
                 WHERE user_id = ?1 AND credential_id = ?2",
                params![user_id, stored.credential_id.clone(), json],
            )
            .await
            .map_err(io::Error::other)?;
        if changed == 0 {
            Err(io::Error::new(io::ErrorKind::NotFound, "passkey not found"))
        } else {
            Ok(())
        }
    }
}

pub fn session_cookie(token: &SessionToken, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!(
        "{SESSION_COOKIE}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}",
        token.token, token.max_age_seconds, secure
    )
}

pub fn expired_session_cookie(secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}")
}

pub fn cookie_value(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(axum::http::header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(key, value)| (key == name).then(|| value.to_owned()))
}

fn row_to_user(row: Option<libsql::Row>) -> io::Result<Option<User>> {
    row.map(|row| {
        Ok(User {
            id: row.get(0).map_err(io::Error::other)?,
            username: row.get(1).map_err(io::Error::other)?,
            display_name: row.get(2).map_err(io::Error::other)?,
        })
    })
    .transpose()
}

fn hash_password(password: &str) -> io::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    argon2()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(invalid_data)
}

fn verify_password_hash(hash: &str, password: &str) -> io::Result<bool> {
    let parsed = PasswordHash::new(hash).map_err(invalid_data)?;
    Ok(argon2()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

fn argon2<'a>() -> Argon2<'a> {
    Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::default())
}

fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn secret_hash(secret: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(secret.as_bytes()))
}

fn unix_now() -> io::Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(io::Error::other)
}

fn normalize_username(username: &str) -> io::Result<String> {
    let username = username.trim().to_ascii_lowercase();
    if !(3..=64).contains(&username.len())
        || !username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(invalid_config(
            "username must be 3-64 ASCII letters, numbers, dots, dashes, or underscores",
        ));
    }
    Ok(username)
}

fn validate_display_name(display_name: &str) -> io::Result<String> {
    let display_name = display_name.trim();
    if display_name.is_empty() || display_name.chars().count() > 100 {
        return Err(invalid_config("display name must be 1-100 characters"));
    }
    Ok(display_name.to_owned())
}

fn validate_password(password: &str) -> io::Result<()> {
    if password.chars().count() < 12 || password.len() > 1024 {
        return Err(invalid_config("password must be 12-1024 characters"));
    }
    Ok(())
}

fn validate_passkey_label(label: &str) -> io::Result<String> {
    let label = label.trim();
    if label.is_empty() || label.chars().count() > 100 {
        return Err(invalid_config("passkey label must be 1-100 characters"));
    }
    Ok(label.to_owned())
}

fn invalid_config(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn invalid_data(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{AuthStore, SESSION_COOKIE, cookie_value, expired_session_cookie, session_cookie};
    use axum::http::{HeaderMap, HeaderValue, header};
    use std::time::Duration;

    async fn fixture(name: &str) -> (std::path::PathBuf, AuthStore) {
        let root =
            std::env::temp_dir().join(format!("daily-mirror-auth-{name}-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&root).await;
        tokio::fs::create_dir_all(&root).await.unwrap();
        let store = AuthStore::local(root.join("auth.db").to_string_lossy().into_owned());
        (root, store)
    }

    #[tokio::test]
    async fn password_login_and_month_session_lifecycle() {
        let (root, store) = fixture("session").await;
        let user = store
            .create_user("Drew", "Drew", "a-good-test-password")
            .await
            .unwrap();
        assert_eq!(user.username, "drew");
        assert!(
            store
                .verify_password("drew", "a-good-test-password")
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .verify_password("drew", "wrong-password")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .verify_password("missing", "wrong-password")
                .await
                .unwrap()
                .is_none()
        );

        let session = store.create_session(&user.id).await.unwrap();
        assert_eq!(session.max_age_seconds, 30 * 24 * 60 * 60);
        assert_eq!(
            store
                .authenticate_session(&session.token)
                .await
                .unwrap()
                .unwrap()
                .id,
            user.id
        );
        store.revoke_session(&session.token).await.unwrap();
        assert!(
            store
                .authenticate_session(&session.token)
                .await
                .unwrap()
                .is_none()
        );
        drop(store);
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn expired_sessions_and_consumed_ceremonies_cannot_be_replayed() {
        let (root, store) = fixture("expiry").await;
        let user = store
            .create_user("drew", "Drew", "a-good-test-password")
            .await
            .unwrap();
        let expired = store
            .create_session_for(&user.id, Duration::ZERO)
            .await
            .unwrap();
        assert!(
            store
                .authenticate_session(&expired.token)
                .await
                .unwrap()
                .is_none()
        );

        let ceremony = store
            .store_ceremony("passkey-login", &user.id, "{\"state\":true}")
            .await
            .unwrap();
        let first = store
            .take_ceremony(&ceremony, "passkey-login")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.user_id, user.id);
        assert_eq!(first.state_json, "{\"state\":true}");
        assert!(
            store
                .take_ceremony(&ceremony, "passkey-login")
                .await
                .unwrap()
                .is_none()
        );
        drop(store);
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn repeated_login_failures_are_durably_throttled_and_can_be_cleared() {
        let (root, store) = fixture("login-limit").await;
        let scope = "drew\0test-client";
        for _ in 0..super::LOGIN_MAX_FAILURES - 1 {
            store.record_login_failure(scope).await.unwrap();
            assert!(store.login_retry_after(scope).await.unwrap().is_none());
        }
        store.record_login_failure(scope).await.unwrap();
        let retry_after = store.login_retry_after(scope).await.unwrap().unwrap();
        assert!(retry_after > 0);
        assert!(retry_after <= super::LOGIN_BLOCK.as_secs());

        store.clear_login_failures(scope).await.unwrap();
        assert!(store.login_retry_after(scope).await.unwrap().is_none());
        drop(store);
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[test]
    fn session_cookies_are_http_only_scoped_and_parseable() {
        let token = super::SessionToken {
            token: "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG".to_owned(),
            max_age_seconds: 123,
        };
        let cookie = session_cookie(&token, true);
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Lax"));
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("Max-Age=123"));
        assert!(expired_session_cookie(true).contains("Max-Age=0"));

        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("other=1; {SESSION_COOKIE}={}", token.token)).unwrap(),
        );
        assert_eq!(cookie_value(&headers, SESSION_COOKIE), Some(token.token));
    }
}
