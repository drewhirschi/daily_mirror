use axum::{
    Extension,
    extract::Path,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};

use crate::photos::PhotoStore;

const PRIVATE_IMMUTABLE: &str = "private, max-age=31536000, immutable";

pub async fn get(Extension(store): Extension<PhotoStore>, Path(id): Path<String>) -> Response {
    match store.read_thumbnail(&id).await {
        Ok(Some(webp)) => thumbnail_response(webp),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
            StatusCode::BAD_REQUEST.into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn thumbnail_response(webp: Vec<u8>) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "image/webp"),
            (header::CACHE_CONTROL, PRIVATE_IMMUTABLE),
        ],
        webp,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use axum::http::{StatusCode, header};

    use super::{PRIVATE_IMMUTABLE, thumbnail_response};

    #[test]
    fn thumbnails_are_private_and_immutably_cached_by_revisioned_url() {
        let response = thumbnail_response(vec![1, 2, 3]);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/webp");
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            PRIVATE_IMMUTABLE
        );
    }
}
