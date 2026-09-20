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

use crate::auth::{User, random_token, secret_hash};
use crate::processing::ProcessingQueue;

/// Wire timestamps are RFC 3339. SQLite stores `YYYY-MM-DD HH:MM:SS` in UTC,
/// which is lexicographically ordered and therefore safe to compare directly.
const RFC3339: &str = "strftime('%Y-%m-%dT%H:%M:%SZ', {column})";

/// Shown to a signed-in account that belongs to no household, so the app can
/// say what is wrong instead of reporting an opaque failure.
pub const NO_HOUSEHOLD: &str =
    "This account has no household. Sign up again or ask an administrator to link one.";

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

    /// Confirm the shared database is open and at the schema version this
    /// build expects. `devices`, `device_claim_tokens` and `household_users`
    /// are created by the migrations in `server/migrations/`.
    pub(crate) async fn ensure_schema(&self) -> io::Result<()> {
        self.schema
            .get_or_try_init(|| async { self.queue.ensure_schema().await })
            .await
            .copied()
    }

    /// The firmware version recorded for a paired device, so an upload that
    /// does not report its own version still lands on the photo row with the
    /// version that took it.
    pub async fn firmware_version(&self, device_id: &str) -> io::Result<Option<String>> {
        self.ensure_schema().await?;
        let connection = self.queue.catalog.connection().await?;
        let mut rows = connection
            .query(
                "SELECT firmware_version FROM devices WHERE device_id = ?1",
                params![device_id],
            )
            .await
            .map_err(io::Error::other)?;
        let Some(row) = rows.next().await.map_err(io::Error::other)? else {
            return Ok(None);
        };
        row.get::<Option<String>>(0).map_err(io::Error::other)
    }

    /// The household a signed-in user pairs devices into.
    ///
    /// Onboarding is the source of truth: the account's `users.household_id`
    /// decides, so devices pair into exactly the household the Household
    /// screen shows, and `household_users` is kept in step.
    ///
    /// This server is heading for real multi-tenancy, so it never *guesses* a
    /// household. An account that has none is refused with a clear conflict
    /// rather than being quietly attached to whichever household happens to
    /// exist — that could hand one family's cameras to another. Such accounts
    /// predate onboarding; `daily-mirror-onboarding link-household` links one.
    ///
    /// An existing `household_users` row is still honoured, because it is an
    /// explicit binding that today's paired cameras depend on.
    pub async fn household_for_user(&self, user: &User) -> io::Result<String> {
        self.household_for_user_or_none(user)
            .await?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, NO_HOUSEHOLD))
    }

    /// `None` when the account belongs to no household at all.
    pub async fn household_for_user_or_none(&self, user: &User) -> io::Result<Option<String>> {
        self.ensure_schema().await?;
        let connection = self.queue.catalog.connection().await?;
        if let Some(household_id) = user.household_id.as_deref() {
            self.bind_household_user(&connection, &user.id, household_id)
                .await?;
            return Ok(Some(household_id.to_owned()));
        }
        scalar(
            &connection,
            "SELECT household_id FROM household_users WHERE user_id = ?1",
            params![user.id.clone()],
        )
        .await
    }

    /// Move an account's legacy pairing binding onto a household. The
    /// `link-household` admin command calls this so a migrated account's
    /// cameras follow it to its onboarding household.
    pub async fn bind_user_to_household(
        &self,
        user_id: &str,
        household_id: &str,
    ) -> io::Result<()> {
        self.ensure_schema().await?;
        let connection = self.queue.catalog.connection().await?;
        self.bind_household_user(&connection, user_id, household_id)
            .await
    }

    /// Record (or correct) the legacy account-to-household binding so the two
    /// resolution paths always agree.
    async fn bind_household_user(
        &self,
        connection: &libsql::Connection,
        user_id: &str,
        household_id: &str,
    ) -> io::Result<()> {
        connection
            .execute(
                "INSERT INTO household_users (user_id, household_id) VALUES (?1, ?2)
                 ON CONFLICT(user_id) DO UPDATE SET household_id = excluded.household_id",
                params![user_id, household_id],
            )
            .await
            .map_err(io::Error::other)?;
        Ok(())
    }

    pub async fn mint_claim_token(
        &self,
        user: &User,
        server_url: &str,
    ) -> io::Result<ClaimTokenGrant> {
        self.mint_claim_token_with_ttl(user, server_url, CLAIM_TOKEN_TTL_SECONDS as i64)
            .await
    }

    /// Mint against a household the caller has already resolved, so the route
    /// can report "no household" separately from a storage failure.
    pub async fn mint_claim_token_for(
        &self,
        household_id: &str,
        user_id: &str,
        server_url: &str,
    ) -> io::Result<ClaimTokenGrant> {
        self.mint_for(
            household_id,
            user_id,
            server_url,
            CLAIM_TOKEN_TTL_SECONDS as i64,
        )
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
        self.mint_for(&household_id, &user.id, server_url, ttl_seconds)
            .await
    }

    async fn mint_for(
        &self,
        household_id: &str,
        user_id: &str,
        server_url: &str,
        ttl_seconds: i64,
    ) -> io::Result<ClaimTokenGrant> {
        self.ensure_schema().await?;
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
                params![secret_hash(&claim_token), household_id, user_id, modifier],
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

/// Resolve the caller's household for a route, turning "no household" into a
/// 409 the app can explain rather than an opaque server error.
pub async fn require_household(
    registry: &DeviceRegistry,
    user: &User,
) -> Result<String, axum::response::Response> {
    match registry.household_for_user_or_none(user).await {
        Ok(Some(household_id)) => Ok(household_id),
        Ok(None) => Err(crate::auth_http::error(StatusCode::CONFLICT, NO_HOUSEHOLD)),
        Err(_) => Err(crate::auth_http::error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Device service unavailable",
        )),
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

        /// An account with no household, as every account was before
        /// onboarding existed.
        async fn user(&self, username: &str) -> User {
            self.auth
                .create_user(username, username, "strong-test-password")
                .await
                .unwrap()
        }

        /// An onboarded account: it owns a household of its own.
        async fn linked_user(&self, username: &str) -> User {
            let user = self.user(username).await;
            let household = self
                .registry
                .queue
                .create_household(&format!("{username}'s home"), 4)
                .await
                .unwrap();
            let person = self.registry.queue.create_person(username).await.unwrap();
            self.auth
                .link_household(&user.id, &household.id, &person.id)
                .await
                .unwrap();
            self.auth.user_by_id(&user.id).await.unwrap().unwrap()
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
        let user = fixture.linked_user("claimer").await;
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
        let user = fixture.linked_user("expirer").await;
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
        let owner = fixture.linked_user("owner").await;
        let stranger = fixture.linked_user("stranger").await;
        // Two onboarded accounts own two different households.
        let owned = fixture.registry.household_for_user(&owner).await.unwrap();
        let other = fixture
            .registry
            .household_for_user(&stranger)
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
    async fn an_onboarded_account_pairs_into_its_own_household() {
        let fixture = Fixture::new("onboarded").await;
        let first = fixture.linked_user("first").await;
        let second = fixture.linked_user("second").await;
        let own = second.household_id.clone().unwrap();
        assert_ne!(own, first.household_id.clone().unwrap());
        assert_eq!(
            fixture.registry.household_for_user(&second).await.unwrap(),
            own
        );

        // The legacy pairing table is corrected rather than left disagreeing,
        // so both resolution paths always name the same household.
        let connection = fixture.registry.queue.catalog.connection().await.unwrap();
        assert_eq!(
            super::scalar(
                &connection,
                "SELECT household_id FROM household_users WHERE user_id = ?1",
                libsql::params![second.id.clone()],
            )
            .await
            .unwrap()
            .as_deref(),
            Some(own.as_str())
        );
    }

    /// Multi-tenancy is the plan, so a household is never guessed. The old
    /// behaviour bound every account to whichever household happened to exist
    /// first, which on a shared deployment hands one family's cameras to
    /// another.
    #[tokio::test]
    async fn an_account_without_a_household_is_refused_rather_than_guessed() {
        let fixture = Fixture::new("household").await;
        let linked = fixture.linked_user("linked").await;
        let occupied = fixture.registry.household_for_user(&linked).await.unwrap();

        let stranger = fixture.user("stranger").await;
        assert_eq!(
            fixture
                .registry
                .household_for_user_or_none(&stranger)
                .await
                .unwrap(),
            None,
            "an unlinked account must not inherit {occupied}"
        );
        let refused = fixture
            .registry
            .household_for_user(&stranger)
            .await
            .unwrap_err();
        assert_eq!(refused.kind(), std::io::ErrorKind::NotFound);
        assert!(
            fixture
                .registry
                .mint_claim_token(&stranger, "https://mirror.example")
                .await
                .is_err()
        );
        // No household was invented on their behalf.
        let connection = fixture.registry.queue.catalog.connection().await.unwrap();
        let mut rows = connection
            .query("SELECT COUNT(*) FROM households", ())
            .await
            .unwrap();
        assert_eq!(
            rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
            1
        );
        drop(rows);

        // An explicit legacy binding is still honoured, so cameras paired
        // before onboarding keep working until link-household runs.
        connection
            .execute(
                "INSERT INTO household_users (user_id, household_id) VALUES (?1, ?2)",
                libsql::params![stranger.id.clone(), occupied.clone()],
            )
            .await
            .unwrap();
        assert_eq!(
            fixture
                .registry
                .household_for_user(&stranger)
                .await
                .unwrap(),
            occupied
        );
    }
}
