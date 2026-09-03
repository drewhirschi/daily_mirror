use axum::{
    Extension,
    extract::Path,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use crate::photos::{PhotoRead, PhotoStore};
use crate::catalog::PhotoCatalog;
use crate::processing::ProcessingQueue;

pub async fn get(Extension(store): Extension<PhotoStore>, Path(id): Path<String>) -> Response {
    match store.read(&id).await {
        Ok(Some(PhotoRead::Bytes(jpeg))) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "image/jpeg"),
                (
                    header::CACHE_CONTROL,
                    "private, max-age=31536000, immutable",
                ),
            ],
            jpeg,
        )
            .into_response(),
        Ok(Some(PhotoRead::Redirect(url))) => (
            StatusCode::TEMPORARY_REDIRECT,
            [
                (header::LOCATION, url),
                (header::CACHE_CONTROL, "private, no-store".to_owned()),
            ],
        )
            .into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
            StatusCode::BAD_REQUEST.into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize)]
pub struct RotatePhoto {
    degrees: i16,
}

pub async fn patch(
    Extension(store): Extension<PhotoStore>,
    Extension(catalog): Extension<PhotoCatalog>,
    Extension(processing): Extension<ProcessingQueue>,
    Path(id): Path<String>,
    Json(edit): Json<RotatePhoto>,
) -> StatusCode {
    match store.rotate(&id, edit.degrees).await {
        Ok(byte_size) => match catalog.record_rotation(&id, edit.degrees, byte_size).await {
            Ok(()) => match processing.reset_photo(&id).await {
                Ok(()) => StatusCode::NO_CONTENT,
                Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
            },
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
        },
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub async fn delete(
    Extension(store): Extension<PhotoStore>,
    Extension(catalog): Extension<PhotoCatalog>,
    Extension(processing): Extension<ProcessingQueue>,
    Path(id): Path<String>,
) -> StatusCode {
    match store.delete(&id).await {
        Ok(true) => match catalog.delete(&id).await {
            Ok(()) => match processing.delete_photo(&id).await {
                Ok(()) => StatusCode::NO_CONTENT,
                Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
            },
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
        },
        Ok(false) => StatusCode::NOT_FOUND,
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
