use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::photos::PhotoStore;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct Health {
    pub status: &'static str,
    pub software_version: &'static str,
    pub storage_backend: &'static str,
}

#[nextrs::api]
pub async fn get(Extension(store): Extension<PhotoStore>) -> Json<Health> {
    Json(Health {
        status: "ok",
        software_version: env!("CARGO_PKG_VERSION"),
        storage_backend: store.backend_name(),
    })
}
