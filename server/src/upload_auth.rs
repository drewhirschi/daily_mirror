use std::io;

use axum::http::{HeaderMap, StatusCode, header};

use crate::photos::PhotoStore;

pub fn authorize(headers: &HeaderMap) -> Result<(), StatusCode> {
    let Ok(expected) = std::env::var("DAILY_MIRROR_UPLOAD_TOKEN") else {
        return Ok(());
    };
    if expected.is_empty() {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    let supplied = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if supplied == Some(expected.as_str()) {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

pub fn validate_configuration(store: &PhotoStore) -> io::Result<()> {
    if store.is_remote()
        && std::env::var("DAILY_MIRROR_UPLOAD_TOKEN")
            .ok()
            .is_none_or(|token| token.is_empty())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "DAILY_MIRROR_UPLOAD_TOKEN must be set when R2 storage is enabled",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, header};

    use super::authorize;

    #[test]
    fn bearer_token_is_required_when_configured() {
        // Environment mutation is process-global. Use a unique value and restore it so this
        // test remains friendly to callers running the suite with an existing local token.
        let previous = std::env::var("DAILY_MIRROR_UPLOAD_TOKEN").ok();
        unsafe { std::env::set_var("DAILY_MIRROR_UPLOAD_TOKEN", "test-upload-token") };

        let mut headers = HeaderMap::new();
        assert!(authorize(&headers).is_err());
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer test-upload-token"),
        );
        assert!(authorize(&headers).is_ok());

        match previous {
            Some(value) => unsafe { std::env::set_var("DAILY_MIRROR_UPLOAD_TOKEN", value) },
            None => unsafe { std::env::remove_var("DAILY_MIRROR_UPLOAD_TOKEN") },
        }
    }
}
