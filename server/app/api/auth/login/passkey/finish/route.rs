use axum::{Extension, Json, http::StatusCode, response::Response};
use serde::Deserialize;
use webauthn_rs::prelude::PublicKeyCredential;

use crate::{auth::AuthStore, auth_http, passkeys::PasskeyService};

#[derive(Deserialize)]
pub struct PasskeyLoginFinish {
    ceremony_id: String,
    credential: PublicKeyCredential,
}

pub async fn post(
    Extension(store): Extension<AuthStore>,
    Extension(passkeys): Extension<PasskeyService>,
    Json(request): Json<PasskeyLoginFinish>,
) -> Response {
    let user = match passkeys
        .finish_authentication(&store, &request.ceremony_id, &request.credential)
        .await
    {
        Ok(user) => user,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::NotFound
            ) =>
        {
            return auth_http::error(StatusCode::UNAUTHORIZED, "Passkey authentication failed");
        }
        Err(_) => return auth_http::internal_error(),
    };
    match store.create_session(&user.id).await {
        Ok(token) => auth_http::logged_in(user, &token, passkeys.secure_cookies()),
        Err(_) => auth_http::internal_error(),
    }
}
