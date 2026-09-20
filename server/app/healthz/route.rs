use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::photos::PhotoStore;
use crate::schema_gate::{SchemaGate, SchemaReport};

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct Health {
    pub status: &'static str,
    pub software_version: &'static str,
    pub storage_backend: &'static str,
    /// Applied schema version, the version this build expects, and how many
    /// migrations are still pending. `status` is `degraded` when they differ.
    pub schema: SchemaReport,
}

#[nextrs::api]
pub async fn get(
    Extension(store): Extension<PhotoStore>,
    Extension(gate): Extension<SchemaGate>,
) -> Json<Health> {
    let schema = gate.report().await;
    Json(Health {
        // Health stays reachable when the schema is behind: that is precisely
        // when someone needs to read this endpoint.
        status: if schema.is_ready() { "ok" } else { "degraded" },
        software_version: env!("CARGO_PKG_VERSION"),
        storage_backend: store.backend_name(),
        schema,
    })
}
