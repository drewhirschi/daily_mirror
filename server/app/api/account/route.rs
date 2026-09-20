use axum::{Extension, response::Response};

use crate::{
    auth::{AuthStore, User, request_session_token},
    auth_http,
    onboarding,
    passkeys::PasskeyService,
    photos::PhotoStore,
    processing::ProcessingQueue,
};

/// Deletes the signed-in account, as App Review guideline 5.1.1(v) requires.
///
/// The session is revoked first so a half-finished delete can never leave a
/// usable token pointing at a deleted account, and the browser cookie is
/// expired on the way out.
#[nextrs::api(responses((status = 204, description = "Account deleted")))]
pub async fn delete(
    Extension(auth): Extension<AuthStore>,
    Extension(queue): Extension<ProcessingQueue>,
    Extension(store): Extension<PhotoStore>,
    Extension(passkeys): Extension<PasskeyService>,
    Extension(user): Extension<User>,
    headers: axum::http::HeaderMap,
) -> Response {
    if let Err(error) = onboarding::delete_account(&queue, &auth, &store, &user).await {
        return onboarding::http_error(error);
    }
    if let Some(token) = request_session_token(&headers) {
        // The account row is already gone; a failure to tidy the session row
        // must not tell the caller the deletion failed.
        let _ = auth.revoke_session(&token).await;
    }
    auth_http::logged_out(passkeys.secure_cookies())
}
