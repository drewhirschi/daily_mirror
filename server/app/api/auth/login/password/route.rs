use axum::{Extension, Json, http::HeaderMap, response::Response};

use crate::{auth::AuthStore, auth_http::{self, PasswordLogin}, passkeys::PasskeyService};

pub async fn post(
    Extension(store): Extension<AuthStore>,
    Extension(passkeys): Extension<PasskeyService>,
    headers: HeaderMap,
    Json(request): Json<PasswordLogin>,
) -> Response {
    match auth_http::password_session(&store, &headers, &request).await {
        Ok((user, token)) => auth_http::logged_in(user, &token, passkeys.secure_cookies()),
        Err(response) => response,
    }
}
