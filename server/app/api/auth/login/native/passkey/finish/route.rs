use crate::{
    auth::AuthStore,
    auth_http::{self, NativeSession},
    passkeys::PasskeyService,
};
use axum::{
    Extension, Json,
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::Deserialize;
use utoipa::ToSchema;
use webauthn_rs::prelude::PublicKeyCredential;

#[derive(Deserialize, ToSchema)]
pub struct NativePasskeyLoginFinish {
    ceremony_id: String,
    #[schema(value_type = serde_json::Value)]
    credential: PublicKeyCredential,
}

#[nextrs::api(responses((status = 200, body = NativeSession)))]
pub async fn post(
    Extension(store): Extension<AuthStore>,
    Extension(passkeys): Extension<PasskeyService>,
    headers: HeaderMap,
    Json(request): Json<NativePasskeyLoginFinish>,
) -> Response {
    auth_http::no_store(match finish(&store, &passkeys, &headers, &request).await {
        Ok(response) | Err(response) => response,
    })
}

async fn finish(
    store: &AuthStore,
    passkeys: &PasskeyService,
    headers: &HeaderMap,
    request: &NativePasskeyLoginFinish,
) -> Result<Response, Response> {
    let scope = auth_http::login_scope("native-passkey-finish", headers);
    if let Some(retry) = store
        .login_retry_after(&scope)
        .await
        .map_err(|_| auth_http::internal_error())?
    {
        return Err(auth_http::rate_limited(retry));
    }
    let user = passkeys
        .finish_authentication(store, &request.ceremony_id, &request.credential)
        .await
        .map_err(|error| {
            if matches!(
                error.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::NotFound
            ) {
                auth_http::error(StatusCode::UNAUTHORIZED, "Passkey authentication failed")
            } else {
                auth_http::internal_error()
            }
        });
    let user = match user {
        Ok(user) => user,
        Err(response) => {
            if response.status() == StatusCode::UNAUTHORIZED {
                store
                    .record_login_failure(&scope)
                    .await
                    .map_err(|_| auth_http::internal_error())?;
            }
            return Err(response);
        }
    };
    store
        .clear_login_failures(&scope)
        .await
        .map_err(|_| auth_http::internal_error())?;
    store
        .clear_login_failures(&auth_http::login_scope(
            &format!("passkey:{}", user.username),
            headers,
        ))
        .await
        .map_err(|_| auth_http::internal_error())?;
    let token = store
        .create_session(&user.id)
        .await
        .map_err(|_| auth_http::internal_error())?;
    Ok(auth_http::native_logged_in(user, token))
}
