use std::io;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use crate::photos::PhotoStore;

pub async fn protect(request: Request<Body>, next: Next) -> Response {
    if bypasses_view_auth(request.method(), request.uri().path()) {
        return next.run(request).await;
    }

    let Some(password) = configured_password() else {
        return next.run(request).await;
    };
    let username =
        std::env::var("DAILY_MIRROR_VIEW_USERNAME").unwrap_or_else(|_| "daily-mirror".to_owned());
    let expected = format!(
        "Basic {}",
        STANDARD.encode(format!("{username}:{password}"))
    );
    let supplied = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    if supplied == Some(expected.as_str()) {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Basic realm=\"Daily Mirror\"")],
            "Authentication required",
        )
            .into_response()
    }
}

pub fn validate_configuration(store: &PhotoStore) -> io::Result<()> {
    if store.is_remote() && configured_password().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "DAILY_MIRROR_VIEW_PASSWORD must be set when R2 storage is enabled",
        ));
    }
    Ok(())
}

fn configured_password() -> Option<String> {
    std::env::var("DAILY_MIRROR_VIEW_PASSWORD")
        .ok()
        .filter(|password| !password.is_empty())
}

fn bypasses_view_auth(method: &Method, path: &str) -> bool {
    path == "/healthz"
        || (method == Method::GET && path == "/api/maintenance/reconcile")
        || (method == Method::POST
            && (matches!(path, "/api/uploads" | "/api/photos")
                || path.starts_with("/api/uploads/")))
}

#[cfg(test)]
mod tests {
    use axum::http::Method;

    use super::bypasses_view_auth;

    #[test]
    fn health_and_device_writes_bypass_gallery_login() {
        assert!(bypasses_view_auth(&Method::GET, "/healthz"));
        assert!(bypasses_view_auth(
            &Method::GET,
            "/api/maintenance/reconcile"
        ));
        assert!(bypasses_view_auth(&Method::POST, "/api/uploads"));
        assert!(bypasses_view_auth(
            &Method::POST,
            "/api/uploads/20260829T071500Z-def67890"
        ));
        assert!(bypasses_view_auth(&Method::POST, "/api/photos"));
        assert!(!bypasses_view_auth(&Method::GET, "/"));
        assert!(!bypasses_view_auth(&Method::GET, "/api/photos"));
        assert!(!bypasses_view_auth(
            &Method::GET,
            "/api/photos/20260829T071500Z-def67890"
        ));
    }
}
