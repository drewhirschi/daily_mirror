use axum::{
    Extension, Json,
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::Deserialize;

use crate::{auth::AuthStore, auth_http, passkeys::PasskeyService};

#[derive(Deserialize)]
pub struct PasswordLogin {
    username: String,
    password: String,
}

pub async fn post(
    Extension(store): Extension<AuthStore>,
    Extension(passkeys): Extension<PasskeyService>,
    headers: HeaderMap,
    Json(request): Json<PasswordLogin>,
) -> Response {
    let scope = auth_http::login_scope(&request.username, &headers);
    match store.login_retry_after(&scope).await {
        Ok(Some(retry_after)) => return auth_http::rate_limited(retry_after),
        Ok(None) => {}
        Err(_) => return auth_http::internal_error(),
    }
    let user = match store
        .verify_password(&request.username, &request.password)
        .await
    {
        Ok(Some(user)) => user,
        Ok(None) => {
            if store.record_login_failure(&scope).await.is_err() {
                return auth_http::internal_error();
            }
            return auth_http::error(StatusCode::UNAUTHORIZED, "Invalid username or password");
        }
        Err(_) => return auth_http::internal_error(),
    };
    if store.clear_login_failures(&scope).await.is_err() {
        return auth_http::internal_error();
    }
    match store.create_session(&user.id).await {
        Ok(token) => auth_http::logged_in(user, &token, passkeys.secure_cookies()),
        Err(_) => auth_http::internal_error(),
    }
}
