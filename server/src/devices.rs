//! Device pairing: claim-token mint, claim redemption, per-device tokens.
//!
//! The flow is specified in `docs/device-pairing-plan.md`. A signed-in user
//! mints a short-lived, single-use claim token bound to their household. The
//! device trades that token for a long-lived per-device token, which then
//! authenticates its uploads. Only hashes of both secrets are stored, matching
//! how [`crate::auth`] stores session tokens.

use std::io;
use std::sync::Arc;

use axum::http::StatusCode;
use daily_mirror_core::contract::{
    CLAIM_TOKEN_TTL_SECONDS, ClaimTokenGrant, DeviceClaimRequest, DeviceClaimed, DeviceSummary,
};
use libsql::{TransactionBehavior, params};
use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::auth::{User, random_token, secret_hash};
use crate::processing::ProcessingQueue;

/// Wire timestamps are RFC 3339. SQLite stores `YYYY-MM-DD HH:MM:SS` in UTC,
/// which is lexicographically ordered and therefore safe to compare directly.
const RFC3339: &str = "strftime('%Y-%m-%dT%H:%M:%SZ', {column})";

#[derive(Clone, Debug)]
pub struct DeviceRegistry {
    queue: ProcessingQueue,
    schema: Arc<OnceCell<()>>,
}

#[derive(Debug)]
pub enum DeviceError {
    InvalidInput(String),
    /// The claim token is unknown, already consumed, or past its TTL.
    InvalidClaimToken,
    /// The device is already claimed by a different household and was never
    /// released by a full reset.
    HouseholdConflict,
    Storage(io::Error),
}

impl DeviceError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::InvalidInput(_) => StatusCode::BAD_REQUEST,
            Self::InvalidClaimToken => StatusCode::UNAUTHORIZED,
            Self::HouseholdConflict => StatusCode::CONFLICT,
            Self::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl From<io::Error> for DeviceError {
    fn from(error: io::Error) -> Self {
        Self::Storage(error)
    }
}

impl DeviceRegistry {
    pub fn new(queue: ProcessingQueue) -> Self {
        Self {
            queue,
            schema: Arc::new(OnceCell::new()),
        }
    }

    /// Migrations run the same way the rest of this server migrates: idempotent
    /// `CREATE TABLE IF NOT EXISTS` statements applied once per process against
    /// the shared Turso/libsql database.
    pub(crate) async fn ensure_schema(&self) -> io::Result<()> {
        self.schema
            .get_or_try_init(|| async {
                // Households and their members are owned by the processing
                // schema; claim tokens reference them.
                self.queue.ensure_schema().await?;
                let connection = self.queue.catalog.connection().await?;
                connection
                    .execute_batch(
                        "CREATE TABLE IF NOT EXISTS devices (
                            device_id TEXT PRIMARY KEY,
                            household_id TEXT NOT NULL,
                            device_name TEXT NOT NULL,
                            hardware TEXT NOT NULL,
                            firmware_version TEXT NOT NULL,
                            device_token_hash TEXT NOT NULL UNIQUE,
                            claimed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                            last_seen_at TEXT,
                            released_at TEXT
                        );
                        CREATE INDEX IF NOT EXISTS devices_household
                            ON devices(household_id, claimed_at DESC);
                        CREATE TABLE IF NOT EXISTS device_claim_tokens (
                            token_hash TEXT PRIMARY KEY,
                            household_id TEXT NOT NULL,
                            created_by_user TEXT NOT NULL,
                            expires_at TEXT NOT NULL,
                            consumed_at TEXT,
                            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                        );
                        CREATE INDEX IF NOT EXISTS device_claim_tokens_household
                            ON device_claim_tokens(household_id, expires_at);
                        CREATE TABLE IF NOT EXISTS household_users (
                            user_id TEXT PRIMARY KEY,
                            household_id TEXT NOT NULL,
                            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                        );",
                    )
                    .await
                    .map(|_| ())
                    .map_err(io::Error::other)
            })
            .await
            .copied()
    }

    /// The household a signed-in user pairs devices into.
    ///
    /// TODO(tenancy): this server has no membership model yet — every account
    /// sees every photo. Until one lands, a user is bound to the deployment's
    /// existing household (created on demand) and that binding is recorded in
    /// `household_users`, so device rows already carry the right foreign key
    /// when real multi-tenancy arrives.
    pub async fn household_for_user(&self, user: &User) -> io::Result<String> {
        self.ensure_schema().await?;
        let connection = self.queue.catalog.connection().await?;
        if let Some(id) = scalar(
            &connection,
            "SELECT household_id FROM household_users WHERE user_id = ?1",
            params![user.id.clone()],
        )
        .await?
        {
            return Ok(id);
        }
        let household_id = match scalar(
            &connection,
            "SELECT id FROM households ORDER BY created_at, id LIMIT 1",
            (),
        )
        .await?
        {
            Some(id) => id,
            None => {
                let id = Uuid::new_v4().to_string();
                connection
                    .execute(
                        "INSERT INTO households (id, display_name, grid_size)
                         VALUES (?1, ?2, 4)",
                        params![id.clone(), "Home"],
                    )
                    .await
                    .map_err(io::Error::other)?;
                id
            }
        };
        connection
            .execute(
                "INSERT INTO household_users (user_id, household_id) VALUES (?1, ?2)
                 ON CONFLICT(user_id) DO NOTHING",
                params![user.id.clone(), household_id.clone()],
            )
            .await
            .map_err(io::Error::other)?;
        Ok(household_id)
    }

    pub async fn mint_claim_token(
        &self,
        user: &User,
        server_url: &str,
    ) -> io::Result<ClaimTokenGrant> {
        self.mint_claim_token_with_ttl(user, server_url, CLAIM_TOKEN_TTL_SECONDS as i64)
            .await
    }

    /// Test seam for the expiry path. Negative TTLs mint an already-dead token.
    pub async fn mint_claim_token_with_ttl(
        &self,
        user: &User,
        server_url: &str,
        ttl_seconds: i64,
    ) -> io::Result<ClaimTokenGrant> {
        let household_id = self.household_for_user(user).await?;
        let connection = self.queue.catalog.connection().await?;
        let claim_token = random_token();
        // Bound as a parameter so a signed offset ("-60 seconds") stays valid
        // SQLite modifier syntax.
        let modifier = format!("{ttl_seconds:+} seconds");
        connection
            .execute(
                "INSERT INTO device_claim_tokens
                    (token_hash, household_id, created_by_user, expires_at)
                 VALUES (?1, ?2, ?3, datetime('now', ?4))",
                params![
                    secret_hash(&claim_token),
                    household_id.clone(),
                    user.id.clone(),
                    modifier
                ],
            )
            .await
            .map_err(io::Error::other)?;
        let expires_at = scalar(
            &connection,
            &format!(
                "SELECT {} FROM device_claim_tokens WHERE token_hash = ?1",
                RFC3339.replace("{column}", "expires_at")
            ),
            params![secret_hash(&claim_token)],
        )
        .await?
        .ok_or_else(|| io::Error::other("claim token vanished immediately after insert"))?;
        Ok(ClaimTokenGrant {
            claim_token,
            expires_at,
            server_url: server_url.to_owned(),
        })
    }

    /// Redeem a claim token: bind the device to the household, consume the
    /// token, and issue the device its own long-lived bearer token.
    pub async fn claim(&self, request: &DeviceClaimRequest) -> Result<DeviceClaimed, DeviceError> {
        let device_id = validate_field("device ID", &request.device_id, 128)?;
        let hardware = validate_field("hardware", &request.hardware, 100)?;
        let firmware_version = validate_field("firmware version", &request.firmware_version, 100)?;
        if request.claim_token.is_empty() {
            return Err(DeviceError::InvalidClaimToken);
        }
        self.ensure_schema().await?;
        let connection = self.queue.catalog.connection().await?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .map_err(|error| DeviceError::Storage(io::Error::other(error)))?;

        let token_hash = secret_hash(&request.claim_token);
        let household_id = scalar(
            &transaction,
            "SELECT household_id FROM device_claim_tokens
             WHERE token_hash = ?1 AND consumed_at IS NULL AND expires_at > datetime('now')",
            params![token_hash.clone()],
        )
        .await?
        .ok_or(DeviceError::InvalidClaimToken)?;

        // One household per device. A full reset clears the row's
        // `released_at`, which is what lets a device move houses.
        let existing = scalar(
            &transaction,
            "SELECT household_id FROM devices WHERE device_id = ?1 AND released_at IS NULL",
            params![device_id.clone()],
        )
        .await?;
        if existing.is_some_and(|owner| owner != household_id) {
            // Leave the claim token unconsumed: the user can retry against the
            // right household without minting a new one.
            return Err(DeviceError::HouseholdConflict);
        }

        transaction
            .execute(
                "UPDATE device_claim_tokens SET consumed_at = CURRENT_TIMESTAMP
                 WHERE token_hash = ?1",
                params![token_hash],
            )
            .await
            .map_err(|error| DeviceError::Storage(io::Error::other(error)))?;

        let device_token = random_token();
        let device_name = friendly_device_name(&device_id);
        transaction
            .execute(
                "INSERT INTO devices (
                    device_id, household_id, device_name, hardware,
                    firmware_version, device_token_hash
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(device_id) DO UPDATE SET
                    household_id = excluded.household_id,
                    hardware = excluded.hardware,
                    firmware_version = excluded.firmware_version,
                    device_token_hash = excluded.device_token_hash,
                    claimed_at = CURRENT_TIMESTAMP,
                    released_at = NULL",
                params![
                    device_id.clone(),
                    household_id.clone(),
                    device_name.clone(),
                    hardware,
                    firmware_version,
                    secret_hash(&device_token)
                ],
            )
            .await
            .map_err(|error| DeviceError::Storage(io::Error::other(error)))?;
        // A re-claim keeps the name the household already knows.
        let device_name = scalar(
            &transaction,
            "SELECT device_name FROM devices WHERE device_id = ?1",
            params![device_id],
        )
        .await?
        .unwrap_or(device_name);
        transaction
            .commit()
            .await
            .map_err(|error| DeviceError::Storage(io::Error::other(error)))?;

        Ok(DeviceClaimed {
            device_token,
            household_id,
            device_name,
        })
    }

    /// Resolve a per-device bearer token and record the device as seen.
    /// Returns the device ID, or `None` when the token is not a device token.
    pub async fn authenticate_device(&self, token: &str) -> io::Result<Option<String>> {
        if token.len() < 32 {
            return Ok(None);
        }
        self.ensure_schema().await?;
        let connection = self.queue.catalog.connection().await?;
        let Some(device_id) = scalar(
            &connection,
            "SELECT device_id FROM devices
             WHERE device_token_hash = ?1 AND released_at IS NULL",
            params![secret_hash(token)],
        )
        .await?
        else {
            return Ok(None);
        };
        connection
            .execute(
                "UPDATE devices SET last_seen_at = CURRENT_TIMESTAMP WHERE device_id = ?1",
                params![device_id.clone()],
            )
            .await
            .map_err(io::Error::other)?;
        Ok(Some(device_id))
    }

    pub async fn list(&self, household_id: &str) -> io::Result<Vec<DeviceSummary>> {
        self.ensure_schema().await?;
        let connection = self.queue.catalog.connection().await?;
        let statement = format!(
            "SELECT device_id, device_name, hardware, firmware_version, {claimed}, {seen}
             FROM devices WHERE household_id = ?1 AND released_at IS NULL
             ORDER BY claimed_at DESC, device_id",
            claimed = RFC3339.replace("{column}", "claimed_at"),
            seen = RFC3339.replace("{column}", "last_seen_at"),
        );
        let mut rows = connection
            .query(&statement, params![household_id])
            .await
            .map_err(io::Error::other)?;
        let mut devices = Vec::new();
        while let Some(row) = rows.next().await.map_err(io::Error::other)? {
            devices.push(DeviceSummary {
                device_id: row.get(0).map_err(io::Error::other)?,
                device_name: row.get(1).map_err(io::Error::other)?,
                hardware: row.get(2).map_err(io::Error::other)?,
                firmware_version: row.get(3).map_err(io::Error::other)?,
                claimed_at: row.get(4).map_err(io::Error::other)?,
                last_seen_at: row.get(5).map_err(io::Error::other)?,
            });
        }
        Ok(devices)
    }

    /// Mark a device released so its `device_id` can be claimed elsewhere.
    /// The device signals this after a full reset.
    pub async fn release(&self, device_id: &str) -> io::Result<()> {
        self.ensure_schema().await?;
        let connection = self.queue.catalog.connection().await?;
        connection
            .execute(
                "UPDATE devices SET released_at = CURRENT_TIMESTAMP WHERE device_id = ?1",
                params![device_id],
            )
            .await
            .map_err(io::Error::other)?;
        Ok(())
    }
}

/// "Mirror 4F2A" from the trailing hex of a device ID, so a freshly paired
/// device has a name a household can recognize before anyone renames it.
pub fn friendly_device_name(device_id: &str) -> String {
    let suffix: String = device_id
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if suffix.is_empty() {
        "Mirror".to_owned()
    } else {
        format!("Mirror {}", suffix.to_ascii_uppercase())
    }
}

/// The origin a device should store and use for every later request. Vercel
/// terminates TLS at the edge, so the scheme comes from `x-forwarded-proto`.
pub fn public_origin(headers: &axum::http::HeaderMap) -> Option<String> {
    let host = headers
        .get(axum::http::header::HOST)
        .or_else(|| headers.get("x-forwarded-host"))
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.contains('/'))?;
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| matches!(*value, "http" | "https"))
        .unwrap_or(
            if host.starts_with("localhost") || host.starts_with("127.0.0.1") {
                "http"
            } else {
                "https"
            },
        );
    Some(format!("{scheme}://{host}"))
}

fn validate_field(label: &str, value: &str, max: usize) -> Result<String, DeviceError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max {
        return Err(DeviceError::InvalidInput(format!(
            "{label} must be 1-{max} characters"
        )));
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(DeviceError::InvalidInput(format!(
            "{label} must be ASCII letters, numbers, dots, dashes, colons, or underscores"
        )));
    }
    Ok(value.to_owned())
}

async fn scalar(
    connection: &impl Queryable,
    statement: &str,
    parameters: impl libsql::params::IntoParams,
) -> io::Result<Option<String>> {
    let mut rows = connection
        .query_rows(statement, parameters)
        .await
        .map_err(io::Error::other)?;
    let Some(row) = rows.next().await.map_err(io::Error::other)? else {
        return Ok(None);
    };
    row.get::<Option<String>>(0).map_err(io::Error::other)
}

/// Connections and transactions both answer queries; `scalar` accepts either.
trait Queryable {
    fn query_rows(
        &self,
        statement: &str,
        parameters: impl libsql::params::IntoParams,
    ) -> impl std::future::Future<Output = libsql::Result<libsql::Rows>>;
}

impl Queryable for libsql::Connection {
    async fn query_rows(
        &self,
        statement: &str,
        parameters: impl libsql::params::IntoParams,
    ) -> libsql::Result<libsql::Rows> {
        self.query(statement, parameters).await
    }
}

impl Queryable for libsql::Transaction {
    async fn query_rows(
        &self,
        statement: &str,
        parameters: impl libsql::params::IntoParams,
    ) -> libsql::Result<libsql::Rows> {
        self.query(statement, parameters).await
    }
}

#[cfg(test)]
mod tests {
    use super::{DeviceError, DeviceRegistry, friendly_device_name, public_origin};
    use crate::auth::{AuthStore, User};
    use crate::catalog::PhotoCatalog;
    use crate::processing::ProcessingQueue;
    use daily_mirror_core::contract::DeviceClaimRequest;

    struct Fixture {
        registry: DeviceRegistry,
        auth: AuthStore,
        path: std::path::PathBuf,
    }

    impl Fixture {
        async fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "daily-mirror-devices-{label}-{}.db",
                uuid::Uuid::new_v4()
            ));
            let catalog = PhotoCatalog::local(path.to_string_lossy().into_owned());
            let auth = AuthStore::local(path.to_string_lossy().into_owned());
            Self {
                registry: DeviceRegistry::new(ProcessingQueue::new(catalog)),
                auth,
                path,
            }
        }

        async fn user(&self, username: &str) -> User {
            self.auth
                .create_user(username, username, "strong-test-password")
                .await
                .unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn claim_request(device_id: &str, claim_token: &str) -> DeviceClaimRequest {
        DeviceClaimRequest {
            device_id: device_id.to_owned(),
            claim_token: claim_token.to_owned(),
            firmware_version: "0.1.0".to_owned(),
            hardware: "esp32-p4-imx519".to_owned(),
        }
    }

    #[test]
    fn device_names_come_from_the_identifier_suffix() {
        assert_eq!(friendly_device_name("mirror-abc4f2a"), "Mirror 4F2A");
        assert_eq!(friendly_device_name("ab"), "Mirror AB");
        assert_eq!(friendly_device_name("--"), "Mirror");
    }

    #[test]
    fn public_origin_trusts_the_edge_forwarded_scheme() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(axum::http::header::HOST, "mirror.example".parse().unwrap());
        assert_eq!(
            public_origin(&headers).as_deref(),
            Some("https://mirror.example")
        );
        headers.insert("x-forwarded-proto", "http".parse().unwrap());
        assert_eq!(
            public_origin(&headers).as_deref(),
            Some("http://mirror.example")
        );
        headers.insert(axum::http::header::HOST, "localhost:3000".parse().unwrap());
        headers.remove("x-forwarded-proto");
        assert_eq!(
            public_origin(&headers).as_deref(),
            Some("http://localhost:3000")
        );
        assert_eq!(public_origin(&axum::http::HeaderMap::new()), None);
    }

    #[tokio::test]
    async fn a_claim_consumes_its_token_and_issues_a_device_token() {
        let fixture = Fixture::new("claim").await;
        let user = fixture.user("claimer").await;
        let grant = fixture
            .registry
            .mint_claim_token(&user, "https://mirror.example")
            .await
            .unwrap();
        assert_eq!(grant.server_url, "https://mirror.example");
        assert!(grant.expires_at.ends_with('Z'), "{}", grant.expires_at);

        let claimed = fixture
            .registry
            .claim(&claim_request("mirror-abc4f2a", &grant.claim_token))
            .await
            .unwrap();
        assert_eq!(claimed.device_name, "Mirror 4F2A");
        assert_eq!(
            claimed.household_id,
            fixture.registry.household_for_user(&user).await.unwrap()
        );

        // Single use: the same token cannot claim a second device.
        let replay = fixture
            .registry
            .claim(&claim_request("mirror-second", &grant.claim_token))
            .await
            .unwrap_err();
        assert!(matches!(replay, DeviceError::InvalidClaimToken));

        let devices = fixture.registry.list(&claimed.household_id).await.unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].device_id, "mirror-abc4f2a");
        assert_eq!(devices[0].hardware, "esp32-p4-imx519");
        assert_eq!(devices[0].last_seen_at, None);

        // The device token authenticates and records presence.
        assert_eq!(
            fixture
                .registry
                .authenticate_device(&claimed.device_token)
                .await
                .unwrap()
                .as_deref(),
            Some("mirror-abc4f2a")
        );
        assert!(
            fixture.registry.list(&claimed.household_id).await.unwrap()[0]
                .last_seen_at
                .is_some()
        );
        assert_eq!(
            fixture
                .registry
                .authenticate_device(&"z".repeat(43))
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn an_expired_claim_token_is_rejected() {
        let fixture = Fixture::new("expired").await;
        let user = fixture.user("expirer").await;
        let grant = fixture
            .registry
            .mint_claim_token_with_ttl(&user, "https://mirror.example", -60)
            .await
            .unwrap();
        let error = fixture
            .registry
            .claim(&claim_request("mirror-expired", &grant.claim_token))
            .await
            .unwrap_err();
        assert!(matches!(error, DeviceError::InvalidClaimToken));
        assert_eq!(error.status_code(), axum::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn a_device_held_by_another_household_cannot_be_reclaimed() {
        let fixture = Fixture::new("conflict").await;
        let owner = fixture.user("owner").await;
        let stranger = fixture.user("stranger").await;
        // Bind the owner first so the stranger does not inherit the same
        // deployment household.
        let owned = fixture.registry.household_for_user(&owner).await.unwrap();
        // Give the stranger a household of their own.
        let other = uuid::Uuid::new_v4().to_string();
        let connection = fixture.registry.queue.catalog.connection().await.unwrap();
        connection
            .execute(
                "INSERT INTO households (id, display_name, grid_size) VALUES (?1, 'Other', 4)",
                libsql::params![other.clone()],
            )
            .await
            .unwrap();
        connection
            .execute(
                "INSERT INTO household_users (user_id, household_id) VALUES (?1, ?2)",
                libsql::params![stranger.id.clone(), other.clone()],
            )
            .await
            .unwrap();

        let first = fixture
            .registry
            .mint_claim_token(&owner, "https://mirror.example")
            .await
            .unwrap();
        let held = fixture
            .registry
            .claim(&claim_request("mirror-shared", &first.claim_token))
            .await
            .unwrap();
        assert_eq!(held.household_id, owned);
        assert_ne!(owned, other);

        let second = fixture
            .registry
            .mint_claim_token(&stranger, "https://mirror.example")
            .await
            .unwrap();
        let error = fixture
            .registry
            .claim(&claim_request("mirror-shared", &second.claim_token))
            .await
            .unwrap_err();
        assert!(matches!(error, DeviceError::HouseholdConflict));
        assert_eq!(error.status_code(), axum::http::StatusCode::CONFLICT);
        assert!(fixture.registry.list(&other).await.unwrap().is_empty());

        // A full reset releases the row, and the second household may claim it.
        fixture.registry.release("mirror-shared").await.unwrap();
        let moved = fixture
            .registry
            .claim(&claim_request("mirror-shared", &second.claim_token))
            .await
            .unwrap();
        assert_eq!(moved.household_id, other);
    }

    #[tokio::test]
    async fn every_user_of_this_single_tenant_deployment_shares_one_household() {
        let fixture = Fixture::new("household").await;
        let first = fixture.user("first").await;
        let second = fixture.user("second").await;
        let household = fixture.registry.household_for_user(&first).await.unwrap();
        assert_eq!(
            fixture.registry.household_for_user(&second).await.unwrap(),
            household
        );
        // Stable across calls.
        assert_eq!(
            fixture.registry.household_for_user(&first).await.unwrap(),
            household
        );
    }
}
