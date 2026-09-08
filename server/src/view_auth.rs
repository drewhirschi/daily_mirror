use axum::body::Body;
use axum::extract::State;
use axum::http::{Method, Request, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};

use crate::auth::{AuthStore, request_session_token};

pub async fn protect(
    State(store): State<AuthStore>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    if bypasses_authentication(request.method(), request.uri().path()) {
        return next.run(request).await;
    }

    let user = match request_session_token(request.headers()) {
        Some(token) => store.authenticate_session(&token).await.ok().flatten(),
        None => None,
    };
    if let Some(user) = user {
        let native_session = request.headers().contains_key(header::AUTHORIZATION);
        request.extensions_mut().insert(user);
        let mut response = next.run(request).await;
        // Native clients own their account-scoped media cache. Never retain a
        // second, URL-only copy in the shared NSURLSession HTTP disk cache.
        if native_session {
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
        }
        return response;
    }

    if request.method() == Method::GET && !request.uri().path().starts_with("/api/") {
        Redirect::temporary("/login").into_response()
    } else {
        (StatusCode::UNAUTHORIZED, "Authentication required").into_response()
    }
}

fn bypasses_authentication(method: &Method, path: &str) -> bool {
    path == "/healthz"
        || (method == Method::GET && path == "/.well-known/apple-app-site-association")
        || path == "/login"
        || matches!(
            path,
            "/style.css" | "/favicon.ico" | "/robots.txt" | "/manifest.webmanifest" | "/sw.js"
        )
        || path.starts_with("/icons/")
        || path.starts_with("/dist/")
        || path.starts_with("/api/auth/login/")
        || path.starts_with("/api/processing/")
        || (method == Method::GET && matches!(path, "/api/maintenance/reconcile" | "/api/maintenance/process"))
        || (method == Method::POST
            && (matches!(path, "/api/uploads" | "/api/photos")
                || path.starts_with("/api/uploads/")))
}

#[cfg(test)]
mod tests {
    use axum::http::Method;

    use super::bypasses_authentication;

    #[test]
    fn only_health_login_assets_device_writes_and_cron_are_public() {
        assert!(bypasses_authentication(&Method::GET, "/healthz"));
        assert!(bypasses_authentication(&Method::GET, "/login"));
        assert!(bypasses_authentication(&Method::GET, "/style.css"));
        assert!(bypasses_authentication(
            &Method::GET,
            "/manifest.webmanifest"
        ));
        assert!(bypasses_authentication(&Method::GET, "/sw.js"));
        assert!(bypasses_authentication(
            &Method::GET,
            "/icons/apple-touch-icon.png"
        ));
        assert!(bypasses_authentication(&Method::GET, "/dist/page-login.js"));
        assert!(bypasses_authentication(
            &Method::POST,
            "/api/auth/login/password"
        ));
        assert!(bypasses_authentication(
            &Method::POST,
            "/api/auth/login/passkey/start"
        ));
        assert!(bypasses_authentication(
            &Method::GET,
            "/api/maintenance/reconcile"
        ));
        assert!(bypasses_authentication(&Method::POST, "/api/uploads"));
        assert!(bypasses_authentication(
            &Method::POST,
            "/api/processing/claim"
        ));
        assert!(bypasses_authentication(
            &Method::GET,
            "/api/processing/photos/20260829T071500Z-def67890"
        ));
        assert!(bypasses_authentication(
            &Method::POST,
            "/api/uploads/20260829T071500Z-def67890"
        ));
        assert!(!bypasses_authentication(&Method::GET, "/"));
        assert!(!bypasses_authentication(&Method::GET, "/account"));
        assert!(!bypasses_authentication(&Method::GET, "/api/photos"));
        assert!(!bypasses_authentication(&Method::GET, "/api/admin/faces"));
        assert!(!bypasses_authentication(
            &Method::GET,
            "/api/admin/households"
        ));
        assert!(!bypasses_authentication(
            &Method::POST,
            "/api/admin/households"
        ));
        assert!(!bypasses_authentication(
            &Method::PATCH,
            "/api/admin/households/household-id"
        ));
        assert!(!bypasses_authentication(
            &Method::PATCH,
            "/api/admin/faces/face-id"
        ));
        assert!(!bypasses_authentication(
            &Method::POST,
            "/api/auth/passkeys/register/start"
        ));
        assert!(!bypasses_authentication(&Method::POST, "/api/auth/logout"));
        assert!(!bypasses_authentication(
            &Method::GET,
            "/api/photos/20260829T071500Z-def67890"
        ));
    }
}
