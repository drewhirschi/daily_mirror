use axum::{
    Extension, Json,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use utoipa::ToSchema;

use crate::{
    auth::{AuthStore, User},
    auth_http::{self, PasswordLogin},
};

#[derive(Serialize, ToSchema)]
pub struct NativeSession {
    pub user: User,
    pub token: String,
    pub expires_in_seconds: u64,
}

#[nextrs::api(responses((status = 200, body = NativeSession)))]
pub async fn post(
    Extension(store): Extension<AuthStore>,
    headers: HeaderMap,
    Json(request): Json<PasswordLogin>,
) -> Response {
    let mut response = match auth_http::password_session(&store, &headers, &request).await {
        Ok((user, token)) => Json(NativeSession {
            user,
            token: token.token,
            expires_in_seconds: token.max_age_seconds,
        })
        .into_response(),
        Err(response) => response,
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}
