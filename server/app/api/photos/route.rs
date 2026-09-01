use axum::{
    Extension, Json,
    body::Bytes,
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::photos::{Photo, PhotoStore};
use crate::catalog::PhotoCatalog;
use crate::realtime::RealtimeHub;
use crate::upload_auth;
use crate::upload_flow::gallery_photos;

#[derive(Serialize, Deserialize, ToSchema)]
pub struct PhotoList {
    pub photos: Vec<Photo>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct UploadResponse {
    pub id: String,
    pub url: String,
}

#[nextrs::api]
pub async fn get(
    Extension(catalog): Extension<PhotoCatalog>,
) -> Result<Json<PhotoList>, StatusCode> {
    let photos = gallery_photos(&catalog)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(PhotoList { photos }))
}

pub async fn post(
    Extension(store): Extension<PhotoStore>,
    Extension(catalog): Extension<PhotoCatalog>,
    Extension(realtime): Extension<RealtimeHub>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<UploadResponse>), StatusCode> {
    upload_auth::authorize(&headers)?;
    let id = required_header(&headers, "x-capture-id")?;
    let storage_key = store.storage_key(id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let saved = store
        .save(id, &body)
        .await
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::InvalidData | std::io::ErrorKind::InvalidInput => {
                StatusCode::BAD_REQUEST
            }
            std::io::ErrorKind::Unsupported => StatusCode::UPGRADE_REQUIRED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        })?;
    store
        .ensure_thumbnail(&saved.id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    catalog.register_ready(&saved.id, &storage_key, body.len() as u64).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    catalog.mark_thumbnail_ready(&saved.id).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    realtime.publish_photo("photo.created", &saved.id).await;

    Ok((
        StatusCode::CREATED,
        Json(UploadResponse {
            id: saved.id,
            url: saved.url,
        }),
    ))
}

fn required_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, StatusCode> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .ok_or(StatusCode::BAD_REQUEST)
}
