use axum::{Extension, extract::Path, http::{HeaderMap, StatusCode}};

use crate::{catalog::PhotoCatalog, photos::PhotoStore, upload_auth};

pub async fn post(
    Extension(store): Extension<PhotoStore>,
    Extension(catalog): Extension<PhotoCatalog>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> StatusCode {
    if let Err(status) = upload_auth::authorize(&headers) {
        return status;
    }
    let expected_size = match catalog.expected_size(&id).await {
        Ok(Some(size)) => size,
        Ok(None) => return StatusCode::NOT_FOUND,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR,
    };
    let uploaded_size = match store.uploaded_size(&id).await {
        Ok(Some(size)) => size,
        Ok(None) => return StatusCode::CONFLICT,
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
            return StatusCode::BAD_REQUEST;
        }
        Err(_) => return StatusCode::BAD_GATEWAY,
    };
    if uploaded_size != expected_size {
        return StatusCode::CONFLICT;
    }
    match catalog.mark_ready(&id).await {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
