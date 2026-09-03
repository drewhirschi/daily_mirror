use std::io;

use axum::http::{HeaderMap, StatusCode, header};

const PROCESSOR_TOKEN_ENV: &str = "DAILY_MIRROR_PROCESSOR_TOKEN";

pub fn authorize(headers: &HeaderMap) -> Result<(), StatusCode> {
    let expected = configured_token().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
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

pub fn validate_configuration() -> io::Result<()> {
    if std::env::var("VERCEL_ENV").as_deref() == Ok("production") && configured_token().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{PROCESSOR_TOKEN_ENV} must be set in Vercel production"),
        ));
    }
    Ok(())
}

fn configured_token() -> Option<String> {
    std::env::var(PROCESSOR_TOKEN_ENV)
        .ok()
        .filter(|token| token.len() >= 16)
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, StatusCode, header};

    use super::authorize;

    #[test]
    fn processor_requires_the_exact_bearer_token() {
        let previous = std::env::var("DAILY_MIRROR_PROCESSOR_TOKEN").ok();
        unsafe { std::env::set_var("DAILY_MIRROR_PROCESSOR_TOKEN", "test-processor-token-1234") };

        let mut headers = HeaderMap::new();
        assert_eq!(authorize(&headers), Err(StatusCode::UNAUTHORIZED));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer wrong-processor-token"),
        );
        assert_eq!(authorize(&headers), Err(StatusCode::UNAUTHORIZED));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer test-processor-token-1234"),
        );
        assert!(authorize(&headers).is_ok());

        match previous {
            Some(value) => unsafe { std::env::set_var("DAILY_MIRROR_PROCESSOR_TOKEN", value) },
            None => unsafe { std::env::remove_var("DAILY_MIRROR_PROCESSOR_TOKEN") },
        }
    }
}
