use std::io;

use axum::http::{HeaderMap, StatusCode, header};

pub fn authorize(headers: &HeaderMap) -> Result<(), StatusCode> {
    let secret = configured_secret().ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    authorize_with_secret(headers, &secret)
}

pub fn validate_configuration() -> io::Result<()> {
    if std::env::var("VERCEL_ENV").as_deref() == Ok("production") && configured_secret().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "CRON_SECRET must be set in Vercel production",
        ));
    }
    Ok(())
}

fn configured_secret() -> Option<String> {
    std::env::var("CRON_SECRET")
        .ok()
        .filter(|secret| secret.len() >= 16)
}

fn authorize_with_secret(headers: &HeaderMap, secret: &str) -> Result<(), StatusCode> {
    let supplied = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let expected = format!("Bearer {secret}");
    if supplied == Some(expected.as_str()) {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, StatusCode, header};

    use super::authorize_with_secret;

    #[test]
    fn cron_requires_the_exact_bearer_secret() {
        let mut headers = HeaderMap::new();
        assert_eq!(
            authorize_with_secret(&headers, "test-cron-secret-1234"),
            Err(StatusCode::UNAUTHORIZED)
        );
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer wrong-secret-value"),
        );
        assert_eq!(
            authorize_with_secret(&headers, "test-cron-secret-1234"),
            Err(StatusCode::UNAUTHORIZED)
        );
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer test-cron-secret-1234"),
        );
        assert!(authorize_with_secret(&headers, "test-cron-secret-1234").is_ok());
    }
}
