use axum::{
    Extension, Json,
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::photos::{PhotoStore, UploadTarget};
use crate::catalog::PhotoCatalog;
use crate::upload_auth;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UploadRequest {
    pub capture_id: String,
    pub content_type: String,
    pub content_length: u64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UploadGrant {
    pub method: String,
    pub url: String,
    pub headers: std::collections::BTreeMap<String, String>,
    pub expires_in_seconds: Option<u64>,
    pub complete_url: String,
}

#[nextrs::api]
pub async fn post(
    Extension(store): Extension<PhotoStore>,
    Extension(catalog): Extension<PhotoCatalog>,
    headers: HeaderMap,
    Json(request): Json<UploadRequest>,
) -> Result<Json<UploadGrant>, StatusCode> {
    upload_auth::authorize(&headers)?;
    let storage_key = store.storage_key(&request.capture_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    catalog.reserve(&request.capture_id, &storage_key, request.content_length).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let target = store
        .create_upload(
            &request.capture_id,
            &request.content_type,
            request.content_length,
        )
        .await
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::InvalidData | std::io::ErrorKind::InvalidInput => {
                StatusCode::BAD_REQUEST
            }
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        })?;

    Ok(Json(grant(target, &request.capture_id)))
}

fn grant(target: UploadTarget, id: &str) -> UploadGrant {
    UploadGrant {
        method: target.method,
        url: target.url,
        headers: target.headers,
        expires_in_seconds: target.expires_in_seconds,
        complete_url: format!("/api/uploads/{id}"),
    }
}
