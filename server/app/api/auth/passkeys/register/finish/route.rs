use axum::{Extension, Json, http::StatusCode, response::{IntoResponse, Response}};
use serde::Deserialize;
use webauthn_rs::prelude::RegisterPublicKeyCredential;

use crate::{auth::{AuthStore, User}, auth_http, passkeys::PasskeyService};

#[derive(Deserialize)]
pub struct PasskeyRegistrationFinish {
    ceremony_id: String,
    label: String,
    credential: RegisterPublicKeyCredential,
}

pub async fn post(
    Extension(store): Extension<AuthStore>,
    Extension(user): Extension<User>,
    Extension(passkeys): Extension<PasskeyService>,
    Json(request): Json<PasskeyRegistrationFinish>,
) -> Response {
    match passkeys
        .finish_registration(
            &store,
            &user,
            &request.ceremony_id,
            &request.label,
            &request.credential,
        )
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            auth_http::error(StatusCode::CONFLICT, "That passkey is already registered")
        }
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::InvalidInput
            ) =>
        {
            auth_http::error(StatusCode::BAD_REQUEST, "Passkey registration failed")
        }
        Err(_) => auth_http::internal_error(),
    }
}
