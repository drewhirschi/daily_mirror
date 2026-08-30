use axum::{
    Extension, Json,
    body::Bytes,
    http::{HeaderMap, StatusCode},
};
use std::collections::HashSet;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::photos::{Photo, PhotoStore};
use crate::catalog::PhotoCatalog;
use crate::upload_auth;

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
    Extension(store): Extension<PhotoStore>,
    Extension(catalog): Extension<PhotoCatalog>,
) -> Result<Json<PhotoList>, StatusCode> {
    let mut unresolved = HashSet::new();
    for pending in catalog
        .pending()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        match store
            .uploaded_size(&pending.id)
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?
        {
            Some(size) if size == pending.byte_size => catalog
                .mark_ready(&pending.id)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            _ => {
                unresolved.insert(pending.id);
            }
        }
    }

    if catalog
        .ready_is_empty()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        let stored = store
            .list()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .into_iter()
            .filter(|photo| !unresolved.contains(&photo.id))
            .map(|photo| {
                let key = store.storage_key(&photo.id)?;
                Ok((photo, key))
            })
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        catalog
            .import(&stored)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    let photos = catalog
        .list()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(PhotoList { photos }))
}

pub async fn post(
    Extension(store): Extension<PhotoStore>,
    Extension(catalog): Extension<PhotoCatalog>,
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
    catalog.register_ready(&saved.id, &storage_key, body.len() as u64).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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
