use crate::{
    auth::AuthStore,
    auth_http::{self, NativeSession, PasswordLogin},
};
use axum::{Extension, Json, http::HeaderMap, response::Response};

#[nextrs::api(responses((status = 200, body = NativeSession)))]
pub async fn post(
    Extension(store): Extension<AuthStore>,
    headers: HeaderMap,
    Json(request): Json<PasswordLogin>,
) -> Response {
    match auth_http::password_session(&store, &headers, &request).await {
        Ok((user, token)) => auth_http::native_logged_in(user, token),
        Err(response) => auth_http::no_store(response),
    }
}
