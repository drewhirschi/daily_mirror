//! Versioned schema migrations.
//!
//! The ordered list in [`MIGRATIONS`] is the single source of truth for this
//! database's shape. Nothing else in the server may create, alter or drop a
//! table: request handlers read and write rows only.
//!
//! Each migration is a plain `.sql` file under `server/migrations/`, embedded
//! with `include_str!` so a deployed binary carries its own schema. Applying a
//! migration records a row in `schema_migrations` (version, name, applied_at,
//! checksum) inside the same transaction, so a half-applied migration is never
//! recorded as done.
//!
//! Two rules make this safe for a serverless deployment:
//!
//! * **Every statement is idempotent.** `CREATE ... IF NOT EXISTS`, and
//!   `ALTER TABLE ... ADD COLUMN`, whose "duplicate column name" error the
//!   runner deliberately ignores. A migration that half-applied against a
//!   database libsql could not run transactionally can therefore be re-run.
//! * **Every migration is expand-only.** Vercel keeps serving the previous
//!   build until the new one is live, so the old code must keep working
//!   against the new schema. Add nullable columns and new tables; never drop
//!   or rename one in the same release that stops using it.
//!
//! See `docs/deployment.md` for the authoring checklist.

use std::fmt;
use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

use libsql::{Connection, TransactionBehavior, params};
use sha2::{Digest, Sha256};

/// One ordered schema change.
#[derive(Clone, Copy, Debug)]
pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

impl Migration {
    /// SHA-256 of the migration body, recorded when it is applied so an edit
    /// to an already-applied file is detected instead of silently ignored.
    pub fn checksum(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.sql.as_bytes());
        format!("{:x}", hasher.finalize())
    }
}

/// Ordered, append-only. Never edit an entry that has shipped; add a new one.
pub static MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "baseline",
        sql: include_str!("../migrations/0001_baseline.sql"),
    },
    Migration {
        version: 2,
        name: "capture_metadata",
        sql: include_str!("../migrations/0002_capture_metadata.sql"),
    },
    Migration {
        version: 3,
        name: "account_deletion_requests",
        sql: include_str!("../migrations/0003_account_deletion_requests.sql"),
    },
];

/// The schema version this binary was built against.
pub fn expected_version() -> i64 {
    MIGRATIONS.last().map(|entry| entry.version).unwrap_or(0)
}

/// A migration this database has already recorded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedMigration {
    pub version: i64,
    pub name: String,
    pub applied_at: String,
    pub checksum: String,
}

/// A recorded migration that disagrees with this binary's copy of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Drift {
    /// The file changed after it was applied.
    ChecksumMismatch {
        version: i64,
        name: String,
        recorded: String,
        expected: String,
    },
    /// The database is ahead: it has a version this binary does not know.
    UnknownVersion { version: i64, name: String },
}

impl fmt::Display for Drift {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChecksumMismatch {
                version,
                name,
                recorded,
                expected,
            } => write!(
                formatter,
                "migration {version:04} {name} was applied with checksum {recorded} \
                 but this build carries {expected}"
            ),
            Self::UnknownVersion { version, name } => write!(
                formatter,
                "migration {version:04} {name} was applied by a newer build than this one"
            ),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SchemaStatus {
    pub applied: Vec<AppliedMigration>,
    pub pending: Vec<Migration>,
    pub drift: Vec<Drift>,
    pub expected_version: i64,
}

impl SchemaStatus {
    /// The highest recorded version, or 0 for a database that has never been
    /// migrated.
    pub fn current_version(&self) -> i64 {
        self.applied
            .iter()
            .map(|entry| entry.version)
            .max()
            .unwrap_or(0)
    }

    pub fn is_current(&self) -> bool {
        self.pending.is_empty() && self.drift.is_empty()
    }
}

/// Why a database is not usable by this build.
#[derive(Clone, Debug)]
pub enum SchemaError {
    /// Migrations this build knows have not been applied.
    Pending {
        current_version: i64,
        expected_version: i64,
        pending: usize,
    },
    /// The recorded history does not match this build's migration files.
    Drift(Vec<Drift>),
}

impl SchemaError {
    /// A stable, machine-readable code for API clients and log scrapers.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Pending { .. } => "schema_migration_pending",
            Self::Drift(_) => "schema_migration_drift",
        }
    }
}

impl fmt::Display for SchemaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending {
                current_version,
                expected_version,
                pending,
            } => write!(
                formatter,
                "database schema is at version {current_version} but this build expects \
                 {expected_version} ({pending} migration(s) pending); run \
                 `daily-mirror-migrate up`"
            ),
            Self::Drift(drift) => {
                formatter.write_str("database migration history does not match this build: ")?;
                for (index, entry) in drift.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str("; ")?;
                    }
                    write!(formatter, "{entry}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for SchemaError {}

impl From<SchemaError> for io::Error {
    fn from(error: SchemaError) -> Self {
        io::Error::new(io::ErrorKind::Unsupported, error.to_string())
    }
}

const BOOKKEEPING: &str = "CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    checksum TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS schema_migrations_lock (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    holder TEXT NOT NULL,
    acquired_at INTEGER NOT NULL
);";

/// How long a crashed runner's lock is honoured before another may take it.
const LOCK_STALE_SECONDS: i64 = 300;

/// Create the bookkeeping tables. Only writers call this; [`status`] tolerates
/// their absence so a read-only inspection never touches production.
async fn ensure_bookkeeping(connection: &Connection) -> io::Result<()> {
    connection
        .execute_batch(BOOKKEEPING)
        .await
        .map(|_| ())
        .map_err(io::Error::other)
}

async fn table_exists(connection: &Connection, name: &str) -> io::Result<bool> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![name],
        )
        .await
        .map_err(io::Error::other)?;
    Ok(rows.next().await.map_err(io::Error::other)?.is_some())
}

/// Read the applied/pending picture without writing anything.
pub async fn status(connection: &Connection) -> io::Result<SchemaStatus> {
    let mut applied = Vec::new();
    if table_exists(connection, "schema_migrations").await? {
        let mut rows = connection
            .query(
                "SELECT version, name, applied_at, checksum FROM schema_migrations
                 ORDER BY version",
                (),
            )
            .await
            .map_err(io::Error::other)?;
        while let Some(row) = rows.next().await.map_err(io::Error::other)? {
            applied.push(AppliedMigration {
                version: row.get(0).map_err(io::Error::other)?,
                name: row.get(1).map_err(io::Error::other)?,
                applied_at: row.get(2).map_err(io::Error::other)?,
                checksum: row.get(3).map_err(io::Error::other)?,
            });
        }
    }
    let mut drift = Vec::new();
    for entry in &applied {
        match MIGRATIONS
            .iter()
            .find(|candidate| candidate.version == entry.version)
        {
            Some(known) => {
                let expected = known.checksum();
                if expected != entry.checksum {
                    drift.push(Drift::ChecksumMismatch {
                        version: entry.version,
                        name: entry.name.clone(),
                        recorded: entry.checksum.clone(),
                        expected,
                    });
                }
            }
            None => drift.push(Drift::UnknownVersion {
                version: entry.version,
                name: entry.name.clone(),
            }),
        }
    }
    let pending = MIGRATIONS
        .iter()
        .filter(|candidate| {
            !applied
                .iter()
                .any(|entry| entry.version == candidate.version)
        })
        .copied()
        .collect();
    Ok(SchemaStatus {
        applied,
        pending,
        drift,
        expected_version: expected_version(),
    })
}

/// Fail unless this database is exactly at the version this build expects.
pub async fn verify(connection: &Connection) -> io::Result<Result<SchemaStatus, SchemaError>> {
    let status = status(connection).await?;
    if !status.drift.is_empty() {
        return Ok(Err(SchemaError::Drift(status.drift.clone())));
    }
    if !status.pending.is_empty() {
        return Ok(Err(SchemaError::Pending {
            current_version: status.current_version(),
            expected_version: status.expected_version,
            pending: status.pending.len(),
        }));
    }
    Ok(Ok(status))
}

/// Bring a connection to a usable state, or fail.
///
/// `manage` is true for a local file — a developer's machine or a test — where
/// the store owns its own database and applying migrations is simply how it
/// gets created. It is false for the shared Turso database, where schema
/// changes belong to `daily-mirror-migrate up` in the deploy script and a
/// request-path process must only check.
pub async fn prepare(connection: &Connection, manage: bool) -> io::Result<()> {
    if manage {
        apply(connection, false).await?;
        return Ok(());
    }
    verify(connection)
        .await?
        .map(|_| ())
        .map_err(io::Error::from)
}

/// Apply every pending migration in order, newest last.
///
/// `dry_run` reports what would run and touches nothing. Drift is fatal in
/// both modes: a changed migration file means the two histories have already
/// diverged and only a human can say which is right.
pub async fn apply(
    connection: &Connection,
    dry_run: bool,
) -> io::Result<(SchemaStatus, Vec<Migration>)> {
    let before = status(connection).await?;
    if !before.drift.is_empty() {
        return Err(SchemaError::Drift(before.drift.clone()).into());
    }
    if before.pending.is_empty() || dry_run {
        return Ok((before.clone(), before.pending.clone()));
    }

    ensure_bookkeeping(connection).await?;
    let lock = Lock::acquire(connection).await?;
    // Another runner may have finished between the first read and the lock.
    let fresh = status(connection).await?;
    if !fresh.drift.is_empty() {
        lock.release(connection).await?;
        return Err(SchemaError::Drift(fresh.drift.clone()).into());
    }
    let mut ran = Vec::new();
    for migration in &fresh.pending {
        if let Err(error) = apply_one(connection, migration).await {
            lock.release(connection).await?;
            return Err(error);
        }
        ran.push(*migration);
    }
    lock.release(connection).await?;
    Ok((status(connection).await?, ran))
}

async fn apply_one(connection: &Connection, migration: &Migration) -> io::Result<()> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .await
        .map_err(io::Error::other)?;
    for statement in statements(migration.sql) {
        execute_tolerantly(&transaction, &statement)
            .await
            .map_err(|error| {
                io::Error::other(format!(
                    "migration {:04} {} failed on `{}`: {error}",
                    migration.version,
                    migration.name,
                    first_line(&statement)
                ))
            })?;
    }
    transaction
        .execute(
            "INSERT INTO schema_migrations (version, name, checksum) VALUES (?1, ?2, ?3)",
            params![migration.version, migration.name, migration.checksum()],
        )
        .await
        .map_err(io::Error::other)?;
    transaction.commit().await.map_err(io::Error::other)
}

/// `ALTER TABLE ... ADD COLUMN` is how this schema grows, and re-running it is
/// the documented way a migration stays idempotent, so its duplicate-column
/// error is not a failure. Every other error is.
async fn execute_tolerantly(connection: &Connection, statement: &str) -> io::Result<()> {
    match connection.execute(statement, ()).await {
        Ok(_) => Ok(()),
        Err(error)
            if is_add_column(statement) && error.to_string().contains("duplicate column name") =>
        {
            Ok(())
        }
        Err(error) => Err(io::Error::other(error)),
    }
}

fn is_add_column(statement: &str) -> bool {
    let upper = statement.to_ascii_uppercase();
    upper.starts_with("ALTER TABLE") && upper.contains("ADD COLUMN")
}

fn first_line(statement: &str) -> String {
    statement
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// Split a migration file into statements. libsql's `execute_batch` cannot be
/// used because a single tolerated `ADD COLUMN` failure would abort the batch.
///
/// Handles `--` line comments and single-quoted literals, which is everything
/// this schema uses. Dollar quoting and `BEGIN ... END` trigger bodies are not
/// supported; a migration needing one should be split into several files.
fn statements(sql: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut characters = sql.chars().peekable();
    while let Some(character) = characters.next() {
        if !in_string && character == '-' && characters.peek() == Some(&'-') {
            for skipped in characters.by_ref() {
                if skipped == '\n' {
                    break;
                }
            }
            current.push('\n');
            continue;
        }
        if character == '\'' {
            in_string = !in_string;
        }
        if character == ';' && !in_string {
            push_statement(&mut statements, &mut current);
            continue;
        }
        current.push(character);
    }
    push_statement(&mut statements, &mut current);
    statements
}

fn push_statement(statements: &mut Vec<String>, current: &mut String) {
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        statements.push(trimmed.to_owned());
    }
    current.clear();
}

/// A single-row advisory lock, so two runners (a deploy script and a developer
/// on the same database) cannot interleave migrations.
struct Lock {
    holder: String,
}

impl Lock {
    async fn acquire(connection: &Connection) -> io::Result<Self> {
        let holder = format!(
            "{}:{}",
            std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_owned()),
            std::process::id()
        );
        let now = unix_seconds();
        // A runner that crashed mid-migration must not block the database
        // forever; its statements were idempotent, so retrying is safe.
        connection
            .execute(
                "DELETE FROM schema_migrations_lock WHERE acquired_at < ?1",
                params![now - LOCK_STALE_SECONDS],
            )
            .await
            .map_err(io::Error::other)?;
        connection
            .execute(
                "INSERT INTO schema_migrations_lock (id, holder, acquired_at)
                 VALUES (1, ?1, ?2)",
                params![holder.clone(), now],
            )
            .await
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "another migration runner holds the schema lock; retry shortly",
                )
            })?;
        Ok(Self { holder })
    }

    async fn release(self, connection: &Connection) -> io::Result<()> {
        connection
            .execute(
                "DELETE FROM schema_migrations_lock WHERE id = 1 AND holder = ?1",
                params![self.holder],
            )
            .await
            .map_err(io::Error::other)?;
        Ok(())
    }
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_ordered_and_unique() {
        let mut previous = 0;
        for migration in MIGRATIONS {
            assert!(
                migration.version > previous,
                "migration versions must be ordered and unique"
            );
            previous = migration.version;
        }
        assert_eq!(expected_version(), previous);
    }

    #[test]
    fn statements_split_on_semicolons_outside_strings_and_comments() {
        let split = statements(
            "-- a comment; with a semicolon\n\
             CREATE TABLE t (s TEXT NOT NULL DEFAULT 'a;b');\n\
             ALTER TABLE t ADD COLUMN u TEXT;\n",
        );
        assert_eq!(split.len(), 2);
        assert!(split[0].starts_with("CREATE TABLE t"));
        assert!(split[0].contains("'a;b'"));
        assert!(is_add_column(&split[1]));
    }

    #[test]
    fn every_baseline_statement_is_idempotent() {
        // The baseline runs against a database that already has everything.
        for statement in statements(MIGRATIONS[0].sql) {
            let upper = statement.to_ascii_uppercase();
            assert!(
                upper.contains("IF NOT EXISTS") || is_add_column(&statement),
                "baseline statement is not idempotent: {statement}"
            );
            assert!(
                !upper.starts_with("DROP"),
                "baseline must not drop: {statement}"
            );
        }
    }

    #[test]
    fn checksums_are_stable_and_distinct() {
        assert_eq!(MIGRATIONS[0].checksum(), MIGRATIONS[0].checksum());
        assert_ne!(MIGRATIONS[0].checksum(), MIGRATIONS[1].checksum());
    }

    /// Exactly the DDL the pre-migration server ran on first use, kept here so
    /// the baseline is tested against a database shaped by the old code path
    /// rather than by a tidied-up copy of it.
    const OLD_ENSURE_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            username TEXT NOT NULL COLLATE NOCASE UNIQUE,
            display_name TEXT NOT NULL,
            password_hash TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        ALTER TABLE users ADD COLUMN household_id TEXT;
        ALTER TABLE users ADD COLUMN person_id TEXT;
        ALTER TABLE users ADD COLUMN household_role TEXT NOT NULL DEFAULT 'member';
        CREATE TABLE IF NOT EXISTS photos (
            id TEXT PRIMARY KEY,
            storage_key TEXT NOT NULL,
            captured_at TEXT NOT NULL,
            content_type TEXT NOT NULL DEFAULT 'image/jpeg',
            byte_size INTEGER,
            status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'ready')),
            rotation_degrees INTEGER NOT NULL DEFAULT 0,
            thumbnail_status TEXT NOT NULL DEFAULT 'pending',
            media_revision INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        ALTER TABLE photos ADD COLUMN flipbook_excluded INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE photos ADD COLUMN device_id TEXT;
        ALTER TABLE photos ADD COLUMN source TEXT NOT NULL DEFAULT 'device';
        ALTER TABLE photos ADD COLUMN enrollment_person_id TEXT;";

    async fn scratch(name: &str) -> (Connection, std::path::PathBuf) {
        let directory = std::env::temp_dir().join(format!(
            "daily-mirror-migrations-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        tokio::fs::create_dir_all(&directory).await.unwrap();
        let database = libsql::Builder::new_local(directory.join("db.sqlite"))
            .build()
            .await
            .unwrap();
        (database.connect().unwrap(), directory)
    }

    async fn column_exists(connection: &Connection, table: &str, column: &str) -> bool {
        let mut rows = connection
            .query(&format!("PRAGMA table_info({table})"), ())
            .await
            .unwrap();
        while let Some(row) = rows.next().await.unwrap() {
            if row.get::<String>(1).unwrap() == column {
                return true;
            }
        }
        false
    }

    #[tokio::test]
    async fn a_fresh_database_ends_up_fully_migrated_and_applying_twice_is_a_no_op() {
        let (connection, directory) = scratch("fresh").await;
        let (status, ran) = apply(&connection, false).await.unwrap();
        assert_eq!(ran.len(), MIGRATIONS.len());
        assert_eq!(status.current_version(), expected_version());
        assert!(status.is_current());
        assert!(column_exists(&connection, "photos", "mean_luma").await);
        assert!(verify(&connection).await.unwrap().is_ok());

        let (_, again) = apply(&connection, false).await.unwrap();
        assert!(again.is_empty());
        let _ = tokio::fs::remove_dir_all(directory).await;
    }

    #[tokio::test]
    async fn a_dry_run_reports_the_plan_and_writes_nothing() {
        let (connection, directory) = scratch("dry").await;
        let (status, would_run) = apply(&connection, true).await.unwrap();
        assert_eq!(would_run.len(), MIGRATIONS.len());
        assert_eq!(status.current_version(), 0);
        assert!(
            !table_exists(&connection, "schema_migrations")
                .await
                .unwrap()
        );
        assert!(!table_exists(&connection, "photos").await.unwrap());
        let _ = tokio::fs::remove_dir_all(directory).await;
    }

    /// Production already has the whole schema and no `schema_migrations`.
    #[tokio::test]
    async fn the_baseline_adopts_a_production_shaped_database_without_losing_data() {
        let (connection, directory) = scratch("adopt").await;
        connection.execute_batch(OLD_ENSURE_SCHEMA).await.unwrap();
        connection
            .execute(
                "INSERT INTO photos (id, storage_key, captured_at, byte_size, status)
                 VALUES ('20260915T120000Z-0abcdef0', 'photos/a.jpg', '2026-09-15T12:00:00Z',
                         4096, 'ready')",
                (),
            )
            .await
            .unwrap();
        connection
            .execute(
                "INSERT INTO users (id, username, display_name, password_hash)
                 VALUES ('u1', 'drew', 'Drew', 'hash')",
                (),
            )
            .await
            .unwrap();

        let before = status(&connection).await.unwrap();
        assert_eq!(before.current_version(), 0);
        assert_eq!(before.pending.len(), MIGRATIONS.len());

        let (after, ran) = apply(&connection, false).await.unwrap();
        assert_eq!(ran.len(), MIGRATIONS.len());
        assert_eq!(after.current_version(), expected_version());

        // The rows are untouched and the new columns are simply empty.
        let mut rows = connection
            .query(
                "SELECT byte_size, status, mean_luma FROM photos
                 WHERE id = '20260915T120000Z-0abcdef0'",
                (),
            )
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        assert_eq!(row.get::<i64>(0).unwrap(), 4096);
        assert_eq!(row.get::<String>(1).unwrap(), "ready");
        assert_eq!(row.get::<Option<i64>>(2).unwrap(), None);
        let mut users = connection
            .query("SELECT username, household_role FROM users", ())
            .await
            .unwrap();
        let user = users.next().await.unwrap().unwrap();
        assert_eq!(user.get::<String>(0).unwrap(), "drew");
        assert_eq!(user.get::<String>(1).unwrap(), "member");
        let _ = tokio::fs::remove_dir_all(directory).await;
    }

    #[tokio::test]
    async fn an_edited_migration_is_detected_and_refused_rather_than_reapplied() {
        let (connection, directory) = scratch("checksum").await;
        apply(&connection, false).await.unwrap();
        connection
            .execute(
                "UPDATE schema_migrations SET checksum = 'tampered' WHERE version = 1",
                (),
            )
            .await
            .unwrap();

        let drifted = status(&connection).await.unwrap();
        assert!(matches!(
            drifted.drift.as_slice(),
            [Drift::ChecksumMismatch { version: 1, .. }]
        ));
        assert!(!drifted.is_current());
        let refused = verify(&connection).await.unwrap().unwrap_err();
        assert_eq!(refused.code(), "schema_migration_drift");
        assert!(apply(&connection, false).await.is_err());
        let _ = tokio::fs::remove_dir_all(directory).await;
    }

    #[tokio::test]
    async fn a_database_from_a_newer_build_is_drift_too() {
        let (connection, directory) = scratch("ahead").await;
        apply(&connection, false).await.unwrap();
        connection
            .execute(
                "INSERT INTO schema_migrations (version, name, checksum)
                 VALUES (9999, 'from_the_future', 'x')",
                (),
            )
            .await
            .unwrap();
        assert!(matches!(
            status(&connection).await.unwrap().drift.as_slice(),
            [Drift::UnknownVersion { version: 9999, .. }]
        ));
        let _ = tokio::fs::remove_dir_all(directory).await;
    }

    /// A request-path process must never alter the shared database.
    #[tokio::test]
    async fn preparing_an_unmanaged_database_checks_instead_of_migrating() {
        let (connection, directory) = scratch("unmanaged").await;
        let error = prepare(&connection, false).await.unwrap_err();
        assert!(error.to_string().contains("daily-mirror-migrate up"));
        assert!(!table_exists(&connection, "photos").await.unwrap());

        prepare(&connection, true).await.unwrap();
        prepare(&connection, false).await.unwrap();
        let _ = tokio::fs::remove_dir_all(directory).await;
    }

    #[tokio::test]
    async fn a_held_lock_stops_a_second_runner_and_is_released_afterwards() {
        let (connection, directory) = scratch("lock").await;
        ensure_bookkeeping(&connection).await.unwrap();
        let held = Lock::acquire(&connection).await.unwrap();
        assert!(Lock::acquire(&connection).await.is_err());
        held.release(&connection).await.unwrap();
        assert!(Lock::acquire(&connection).await.is_ok());
        let _ = tokio::fs::remove_dir_all(directory).await;
    }
}
