use axum::{
    Extension, Json,
    http::{HeaderMap, StatusCode},
};

use crate::catalog::PhotoCatalog;
use crate::devices::DeviceRegistry;
use crate::photos::PhotoStore;
use crate::upload_auth;
pub use crate::upload_flow::{UploadGrant, UploadRequest};
use crate::upload_flow::upload_grant;

#[nextrs::api]
pub async fn post(
    Extension(store): Extension<PhotoStore>,
    Extension(catalog): Extension<PhotoCatalog>,
    Extension(registry): Extension<DeviceRegistry>,
    headers: HeaderMap,
    Json(request): Json<UploadRequest>,
) -> Result<Json<UploadGrant>, StatusCode> {
    let principal = upload_auth::authorize(&registry, &headers).await?;
    let storage_key = store.storage_key(&request.capture_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    catalog.reserve_for_device(&request.capture_id, &storage_key, request.content_length, principal.device_id()).await
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

    Ok(Json(upload_grant(
        target,
        format!("/api/uploads/{}", request.capture_id),
    )))
}
