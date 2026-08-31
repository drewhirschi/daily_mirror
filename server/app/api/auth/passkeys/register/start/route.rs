use axum::{Extension, Json, http::StatusCode, response::IntoResponse, response::Response};

use crate::{auth::{AuthStore, User}, auth_http, passkeys::PasskeyService};

pub async fn post(
    Extension(store): Extension<AuthStore>,
    Extension(user): Extension<User>,
    Extension(passkeys): Extension<PasskeyService>,
) -> Response {
    match passkeys.start_registration(&store, &user).await {
        Ok(start) => Json(start).into_response(),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            auth_http::error(StatusCode::CONFLICT, "That passkey is already registered")
        }
        Err(_) => auth_http::internal_error(),
    }
}
