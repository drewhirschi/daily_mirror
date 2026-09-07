use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::auth::{AuthStore, SessionToken, User, expired_session_cookie, session_cookie};

#[derive(Deserialize, utoipa::ToSchema)]
pub struct PasswordLogin {
    pub username: String,
    pub password: String,
}

/// Native and web login deliberately share verification and rate limiting.
pub async fn password_session(
    store: &AuthStore,
    headers: &axum::http::HeaderMap,
    request: &PasswordLogin,
) -> Result<(User, SessionToken), Response> {
    let scope = login_scope(&request.username, headers);
    if let Some(retry_after) = store
        .login_retry_after(&scope)
        .await
        .map_err(|_| internal_error())?
    {
        return Err(rate_limited(retry_after));
    }
    let user = match store
        .verify_password(&request.username, &request.password)
        .await
    {
        Ok(Some(user)) => user,
        Ok(None) => {
            store
                .record_login_failure(&scope)
                .await
                .map_err(|_| internal_error())?;
            return Err(error(
                StatusCode::UNAUTHORIZED,
                "Invalid username or password",
            ));
        }
        Err(_) => return Err(internal_error()),
    };
    store
        .clear_login_failures(&scope)
        .await
        .map_err(|_| internal_error())?;
    let token = store
        .create_session(&user.id)
        .await
        .map_err(|_| internal_error())?;
    Ok((user, token))
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub user: User,
}

#[derive(Serialize)]
struct ErrorResponse<'a> {
    error: &'a str,
}

pub fn logged_in(user: User, token: &SessionToken, secure: bool) -> Response {
    with_cookie(
        Json(LoginResponse { user }).into_response(),
        session_cookie(token, secure),
    )
}

pub fn logged_out(secure: bool) -> Response {
    with_cookie(
        StatusCode::NO_CONTENT.into_response(),
        expired_session_cookie(secure),
    )
}

pub fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(ErrorResponse { error: message })).into_response()
}

pub fn internal_error() -> Response {
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Authentication service unavailable",
    )
}

pub fn rate_limited(retry_after_seconds: u64) -> Response {
    let mut response = error(
        StatusCode::TOO_MANY_REQUESTS,
        "Too many sign-in attempts. Try again later.",
    );
    if let Ok(value) = HeaderValue::from_str(&retry_after_seconds.to_string()) {
        response.headers_mut().insert(header::RETRY_AFTER, value);
    }
    response
}

pub fn login_scope(username: &str, headers: &axum::http::HeaderMap) -> String {
    let client = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next_back())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown-client");
    let username = username
        .trim()
        .to_ascii_lowercase()
        .chars()
        .take(64)
        .collect::<String>();
    let client = client.chars().take(128).collect::<String>();
    format!("{username}\0{client}")
}

fn with_cookie(mut response: Response, cookie: String) -> Response {
    match HeaderValue::from_str(&cookie) {
        Ok(value) => {
            response.headers_mut().insert(header::SET_COOKIE, value);
            response
        }
        Err(_) => internal_error(),
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, StatusCode, header};

    #[test]
    fn login_scope_uses_the_edge_appended_client_address() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("spoofed, 203.0.113.42"),
        );
        assert_eq!(
            super::login_scope(" Drew ", &headers),
            concat!("drew", "\0", "203.0.113.42")
        );
    }

    #[test]
    fn rate_limit_response_is_machine_actionable() {
        let response = super::rate_limited(321);
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers().get(header::RETRY_AFTER).unwrap(), "321");
    }
}
