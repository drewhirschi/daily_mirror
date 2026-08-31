use axum::{Extension, http::HeaderMap, response::Response};

use crate::{
    auth::{AuthStore, SESSION_COOKIE, cookie_value},
    auth_http,
    passkeys::PasskeyService,
};

pub async fn post(
    Extension(store): Extension<AuthStore>,
    Extension(passkeys): Extension<PasskeyService>,
    headers: HeaderMap,
) -> Response {
    if let Some(token) = cookie_value(&headers, SESSION_COOKIE) {
        let _ = store.revoke_session(&token).await;
    }
    auth_http::logged_out(passkeys.secure_cookies())
}
