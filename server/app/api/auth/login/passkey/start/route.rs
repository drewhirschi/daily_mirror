use axum::{
    Extension, Json,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse as _, Response},
};
use serde::Deserialize;

use crate::{
    auth::AuthStore,
    auth_http::{self, DISCOVERABLE_MAX_CHALLENGES, DISCOVERABLE_SCOPE},
    passkeys::PasskeyService,
};

#[derive(Deserialize)]
pub struct PasskeyLoginStart {
    /// Omit (or send an empty string) for a discoverable ceremony, where the
    /// browser offers whichever passkeys it holds for this site.
    #[serde(default)]
    username: Option<String>,
}

pub async fn post(
    Extension(store): Extension<AuthStore>,
    Extension(passkeys): Extension<PasskeyService>,
    headers: HeaderMap,
    Json(request): Json<PasskeyLoginStart>,
) -> Response {
    let username = request
        .username
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(username) = username else {
        return discoverable(&store, &passkeys, &headers).await;
    };
    let user = match store.user_by_username(username).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            return auth_http::error(StatusCode::UNAUTHORIZED, "Passkey login is unavailable");
        }
        Err(_) => return auth_http::internal_error(),
    };
    match passkeys.start_authentication(&store, &user).await {
        Ok(start) => Json(start).into_response(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            auth_http::error(StatusCode::UNAUTHORIZED, "Passkey login is unavailable")
        }
        Err(_) => auth_http::internal_error(),
    }
}

/// Usernameless challenges are limited by client address on a tighter budget,
/// because they name no account for the limiter to key on.
async fn discoverable(
    store: &AuthStore,
    passkeys: &PasskeyService,
    headers: &HeaderMap,
) -> Response {
    let scope = auth_http::login_scope(DISCOVERABLE_SCOPE, headers);
    match store.login_retry_after(&scope).await {
        Ok(Some(retry)) => return auth_http::rate_limited(retry),
        Ok(None) => {}
        Err(_) => return auth_http::internal_error(),
    }
    if store
        .record_login_failure_limited(&scope, DISCOVERABLE_MAX_CHALLENGES)
        .await
        .is_err()
    {
        return auth_http::internal_error();
    }
    match passkeys.start_discoverable_authentication(store).await {
        Ok(start) => Json(start).into_response(),
        Err(_) => auth_http::internal_error(),
    }
}
