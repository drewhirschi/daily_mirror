use axum::{Extension, http::HeaderMap, response::Response};

use crate::{
    auth::{AuthStore, request_session_token},
    auth_http,
    passkeys::PasskeyService,
};

pub async fn post(
    Extension(store): Extension<AuthStore>,
    Extension(passkeys): Extension<PasskeyService>,
    headers: HeaderMap,
) -> Response {
    if let Some(token) = request_session_token(&headers)
        && store.revoke_session(&token).await.is_err()
    {
        return auth_http::internal_error();
    }
    auth_http::logged_out(passkeys.secure_cookies())
}
