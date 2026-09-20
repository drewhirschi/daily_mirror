//! Fail closed when the database is behind the binary.
//!
//! Nothing on a request path migrates any more, so a deploy that reaches
//! production before `daily-mirror-migrate up` did would otherwise read and
//! write columns that do not exist, one confusing 500 at a time. This layer
//! turns that into a single, honest answer: `503` with a machine-readable
//! code, on every database-backed route, until the schema catches up.

use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    Json,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use utoipa::ToSchema;

use crate::catalog::PhotoCatalog;
use crate::migrations;

/// A database that is current stays current for the life of the process, so
/// that answer is cached forever. A database that is behind may be migrated at
/// any moment, so that answer is rechecked often — a serverless invocation is
/// short, and a stuck deployment recovering on its own is worth one query.
const RECHECK_BEHIND: Duration = Duration::from_secs(5);

/// What `/healthz` reports about the schema, and what a refused request says.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize, ToSchema)]
pub struct SchemaReport {
    /// The highest migration this database has recorded.
    pub current_version: i64,
    /// The version this build was compiled against.
    pub expected_version: i64,
    /// How many of this build's migrations have not been applied.
    pub pending: usize,
    /// `ok`, `schema_migration_pending` or `schema_migration_drift`.
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl SchemaReport {
    pub fn is_ready(&self) -> bool {
        self.code == "ok"
    }

    /// Reading the schema at all can fail; an unreachable database is not a
    /// migration problem, so it keeps its own code and does not masquerade as
    /// one.
    fn unavailable(error: &io::Error) -> Self {
        Self {
            current_version: -1,
            expected_version: migrations::expected_version(),
            pending: 0,
            code: "schema_unreadable".to_owned(),
            detail: Some(error.to_string()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SchemaGate {
    catalog: PhotoCatalog,
    cached: Arc<RwLock<Option<(Instant, SchemaReport)>>>,
}

impl SchemaGate {
    pub fn new(catalog: PhotoCatalog) -> Self {
        Self {
            catalog,
            cached: Arc::new(RwLock::new(None)),
        }
    }

    /// The current schema report, cached as described on [`RECHECK_BEHIND`].
    pub async fn report(&self) -> SchemaReport {
        if let Some((checked, report)) = self.cached.read().await.clone()
            && (report.is_ready() || checked.elapsed() < RECHECK_BEHIND)
        {
            return report;
        }
        let report = self.read().await;
        *self.cached.write().await = Some((Instant::now(), report.clone()));
        report
    }

    async fn read(&self) -> SchemaReport {
        // `connection()` itself verifies, and returns an error when the
        // database is behind; go to the raw status so the report can say why.
        let connection = match self.catalog.raw_connection().await {
            Ok(connection) => connection,
            Err(error) => return SchemaReport::unavailable(&error),
        };
        let status = match migrations::status(&connection).await {
            Ok(status) => status,
            Err(error) => return SchemaReport::unavailable(&error),
        };
        let (code, detail) = if !status.drift.is_empty() {
            let error = migrations::SchemaError::Drift(status.drift.clone());
            (error.code().to_owned(), Some(error.to_string()))
        } else if !status.pending.is_empty() {
            let error = migrations::SchemaError::Pending {
                current_version: status.current_version(),
                expected_version: status.expected_version,
                pending: status.pending.len(),
            };
            (error.code().to_owned(), Some(error.to_string()))
        } else {
            ("ok".to_owned(), None)
        };
        SchemaReport {
            current_version: status.current_version(),
            expected_version: status.expected_version,
            pending: status.pending.len(),
            code,
            detail,
        }
    }
}

/// Refuse database-backed API requests while the schema is behind.
///
/// Only `/api/` is gated: the gallery shell, the login page and `/healthz`
/// must stay reachable so an operator can see what is wrong.
pub async fn protect(State(gate): State<SchemaGate>, request: Request, next: Next) -> Response {
    if !request.uri().path().starts_with("/api/") {
        return next.run(request).await;
    }
    let report = gate.report().await;
    if report.is_ready() {
        return next.run(request).await;
    }
    // One line per refused request is too many on a serverless platform, but
    // an operator staring at 503s needs the reason somewhere.
    eprintln!(
        "refusing {} {}: {}",
        request.method(),
        request.uri().path(),
        report.detail.as_deref().unwrap_or(&report.code)
    );
    (StatusCode::SERVICE_UNAVAILABLE, Json(report)).into_response()
}

#[cfg(test)]
mod tests {
    use axum::{Router, body::Body, http::Request, routing::get};
    use tower::ServiceExt;

    use super::*;
    use crate::migrations;

    async fn fixture(name: &str) -> (SchemaGate, std::path::PathBuf) {
        let directory =
            std::env::temp_dir().join(format!("daily-mirror-gate-{name}-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&directory).await.unwrap();
        let catalog =
            PhotoCatalog::local(directory.join("db.sqlite").to_string_lossy().into_owned());
        (SchemaGate::new(catalog), directory)
    }

    fn router(gate: SchemaGate) -> Router {
        Router::new()
            .route("/api/photos", get(|| async { "photos" }))
            .route("/healthz", get(|| async { "health" }))
            .layer(axum::middleware::from_fn_with_state(gate, protect))
    }

    #[tokio::test]
    async fn an_unmigrated_database_fails_closed_with_a_machine_readable_code() {
        let (gate, directory) = fixture("behind").await;
        let report = gate.report().await;
        assert_eq!(report.code, "schema_migration_pending");
        assert_eq!(report.current_version, 0);
        assert_eq!(report.expected_version, migrations::expected_version());
        assert!(report.pending > 0);

        let response = router(gate.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/photos")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(response.into_body(), 8 * 1024)
            .await
            .unwrap();
        let parsed: SchemaReport = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.code, "schema_migration_pending");

        // The operator still needs to reach the health endpoint and the shell.
        let health = router(gate)
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);
        let _ = tokio::fs::remove_dir_all(directory).await;
    }

    #[tokio::test]
    async fn a_migrated_database_is_served_normally() {
        let (gate, directory) = fixture("current").await;
        let connection = gate.catalog.raw_connection().await.unwrap();
        migrations::apply(&connection, false).await.unwrap();

        let report = gate.report().await;
        assert!(report.is_ready(), "{report:?}");
        assert_eq!(report.pending, 0);
        let response = router(gate)
            .oneshot(
                Request::builder()
                    .uri("/api/photos")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let _ = tokio::fs::remove_dir_all(directory).await;
    }
}
