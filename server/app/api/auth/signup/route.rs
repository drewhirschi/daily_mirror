use axum::{Extension, Json, http::HeaderMap, response::Response};

use crate::{
    auth::AuthStore,
    auth_http::{self, NativeSession},
    onboarding::{self, SignupRequest},
    processing::ProcessingQueue,
};

/// Self-service signup. Disabled unless `DAILY_MIRROR_ALLOW_SIGNUP=1`, because
/// every session can still read every household's photos.
#[nextrs::api(responses((status = 200, body = NativeSession)))]
pub async fn post(
    Extension(store): Extension<AuthStore>,
    Extension(queue): Extension<ProcessingQueue>,
    headers: HeaderMap,
    Json(request): Json<SignupRequest>,
) -> Response {
    // Signup shares the password login limiter, so account creation cannot be
    // used to brute force usernames or burn Argon2 time.
    let scope = auth_http::login_scope(&request.username, &headers);
    match store.login_retry_after(&scope).await {
        Ok(Some(retry_after)) => return auth_http::rate_limited(retry_after),
        Ok(None) => {}
        Err(_) => return auth_http::internal_error(),
    }
    if !onboarding::signup_enabled() {
        let _ = store.record_login_failure(&scope).await;
        return failure(&onboarding::OnboardingError::Disabled);
    }
    let user = match onboarding::signup(&store, &queue, &request).await {
        Ok(user) => user,
        Err(error) => {
            let _ = store.record_login_failure(&scope).await;
            return failure(&error);
        }
    };
    if store.clear_login_failures(&scope).await.is_err() {
        return auth_http::internal_error();
    }
    match store.create_session(&user.id).await {
        Ok(token) => auth_http::native_logged_in(user, token),
        Err(_) => auth_http::internal_error(),
    }
}

fn failure(error: &onboarding::OnboardingError) -> Response {
    auth_http::no_store(auth_http::error(error.status_code(), &error.message()))
}
