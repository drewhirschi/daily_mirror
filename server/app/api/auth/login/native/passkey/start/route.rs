use crate::{auth::AuthStore, auth_http, passkeys::PasskeyService};
use axum::{
    Extension, Json,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
pub struct NativePasskeyLoginStart {
    username: String,
}

#[derive(Serialize, ToSchema)]
pub struct NativePasskeyChallenge {
    ceremony_id: String,
    /// WebAuthn request options, including the publicKey challenge.
    options: serde_json::Value,
}

#[nextrs::api(responses((status = 200, body = NativePasskeyChallenge)))]
pub async fn post(
    Extension(store): Extension<AuthStore>,
    Extension(passkeys): Extension<PasskeyService>,
    headers: HeaderMap,
    Json(request): Json<NativePasskeyLoginStart>,
) -> Response {
    auth_http::no_store(match start(&store, &passkeys, &headers, &request).await {
        Ok(result) => Json(result).into_response(),
        Err(response) => response,
    })
}

async fn start(
    store: &AuthStore,
    passkeys: &PasskeyService,
    headers: &HeaderMap,
    request: &NativePasskeyLoginStart,
) -> Result<NativePasskeyChallenge, Response> {
    let scope = auth_http::login_scope(
        &format!("passkey:{}", request.username.trim().to_ascii_lowercase()),
        headers,
    );
    if let Some(retry) = store
        .login_retry_after(&scope)
        .await
        .map_err(|_| auth_http::internal_error())?
    {
        return Err(auth_http::rate_limited(retry));
    }
    // Limit challenge issuance as well as invalid-account attempts.
    store
        .record_login_failure(&scope)
        .await
        .map_err(|_| auth_http::internal_error())?;
    let user = store
        .user_by_username(request.username.trim())
        .await
        .map_err(|_| auth_http::internal_error())?
        .ok_or_else(|| {
            auth_http::error(StatusCode::UNAUTHORIZED, "Passkey login is unavailable")
        })?;
    let result = passkeys
        .start_authentication(store, &user)
        .await
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                auth_http::error(StatusCode::UNAUTHORIZED, "Passkey login is unavailable")
            } else {
                auth_http::internal_error()
            }
        })?;
    Ok(NativePasskeyChallenge {
        ceremony_id: result.ceremony_id,
        options: serde_json::to_value(result.options).map_err(|_| auth_http::internal_error())?,
    })
}
