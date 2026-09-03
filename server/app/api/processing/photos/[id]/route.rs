use axum::{
    Extension,
    extract::Path,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};

use crate::{photos::{PhotoRead, PhotoStore}, processor_auth};

pub async fn get(
    Extension(store): Extension<PhotoStore>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Err(status) = processor_auth::authorize(&headers) {
        return status.into_response();
    }
    match store.read(&id).await {
        Ok(Some(PhotoRead::Bytes(jpeg))) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "image/jpeg"),
                (header::CACHE_CONTROL, "private, no-store"),
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
