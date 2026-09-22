use axum::{
    Extension, Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::{
    auth::{AuthStore, User},
    auth_http,
};

/// The account's open deletion request, or 404 when there is none.
pub async fn get(
    Extension(auth): Extension<AuthStore>,
    Extension(user): Extension<User>,
) -> Response {
    match auth.account_deletion_request(&user.id).await {
        Ok(Some(request)) => auth_http::no_store(Json(request).into_response()),
        Ok(None) => auth_http::no_store(auth_http::error(
            StatusCode::NOT_FOUND,
            "No deletion request",
        )),
        Err(_) => auth_http::internal_error(),
    }
}

/// Ask for the account to be deleted. Nothing is removed here: an operator
/// fulfils the request with `daily-mirror-onboarding delete-account`, which
/// runs `onboarding::delete_account`. Repeating the request returns the
/// original one.
pub async fn post(
    Extension(auth): Extension<AuthStore>,
    Extension(user): Extension<User>,
) -> Response {
    match auth.request_account_deletion(&user).await {
        Ok(request) => {
            auth_http::no_store((StatusCode::ACCEPTED, Json(request)).into_response())
        }
        Err(_) => auth_http::internal_error(),
    }
}
