use axum::{Extension, Json, http::StatusCode, response::Response};
use serde::Deserialize;

use crate::{auth::AuthStore, auth_http, passkeys::PasskeyService};

#[derive(Deserialize)]
pub struct PasskeyLoginStart {
    username: String,
}

pub async fn post(
    Extension(store): Extension<AuthStore>,
    Extension(passkeys): Extension<PasskeyService>,
    Json(request): Json<PasskeyLoginStart>,
) -> Response {
    let user = match store.user_by_username(&request.username).await {
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

use axum::response::IntoResponse as _;
