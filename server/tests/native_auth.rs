use axum::{
    Extension,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
    middleware,
};
use server::{
    auth::{AuthStore, SESSION_COOKIE, request_session_token},
    passkeys::PasskeyService,
    view_auth,
};
use tower::ServiceExt;

#[test]
fn bearer_headers_take_precedence_without_cookie_fallback() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        header::COOKIE,
        format!("{SESSION_COOKIE}=browser-token").parse().unwrap(),
    );
    assert_eq!(
        request_session_token(&headers).as_deref(),
        Some("browser-token")
    );
    headers.insert(
        header::AUTHORIZATION,
        "bEaReR native-token".parse().unwrap(),
    );
    assert_eq!(
        request_session_token(&headers).as_deref(),
        Some("native-token")
    );
    for invalid in ["Bearer", "Bearer ", "Bearer two tokens", "Basic something"] {
        headers.insert(header::AUTHORIZATION, invalid.parse().unwrap());
        assert!(request_session_token(&headers).is_none());
    }
}

#[tokio::test]
async fn native_and_web_share_sessions_rate_limits_and_logout_revocation() {
    let directory =
        std::env::temp_dir().join(format!("daily-mirror-native-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let store = AuthStore::local(directory.join("auth.db").to_string_lossy().into_owned());
    let user = store
        .create_user("native", "Native Test", "strong-test-password")
        .await
        .unwrap();
    let app = nextrs::router::build_router(server::generated_registry())
        .layer(Extension(store.clone()))
        .layer(Extension(PasskeyService::from_env().unwrap()))
        .layer(middleware::from_fn_with_state(
            store.clone(),
            view_auth::protect,
        ));

    let login = |path: &str, password: &str| {
        Request::builder()
            .method("POST")
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({"username": "native", "password": password}).to_string(),
            ))
            .unwrap()
    };
    let response = app
        .clone()
        .oneshot(login("/api/auth/login/native", "strong-test-password"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert!(response.headers().get(header::SET_COOKIE).is_none());
    let session: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
    assert_eq!(session["user"]["id"], user.id);
    assert_eq!(session["expires_in_seconds"], 30 * 24 * 60 * 60);
    let token = session["token"].as_str().unwrap();
    let authorized = |method: &str, path: &str| {
        Request::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap()
    };
    let me = app
        .clone()
        .oneshot(authorized("GET", "/api/auth/me"))
        .await
        .unwrap();
    assert_eq!(me.status(), StatusCode::OK);
    assert_eq!(me.headers()[header::CACHE_CONTROL], "private, no-store");
    assert_eq!(
        app.clone()
            .oneshot(authorized("GET", "/api/auth/passkeys"))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        app.clone()
            .oneshot(authorized("POST", "/api/auth/logout"))
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert!(store.authenticate_session(token).await.unwrap().is_none());
    assert_eq!(
        app.clone()
            .oneshot(authorized("GET", "/api/auth/me"))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/photos/photo/thumbnail")
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );

    let response = app
        .clone()
        .oneshot(login("/api/auth/login/password", "strong-test-password"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().contains_key(header::SET_COOKIE));
    let web: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
    assert!(web.get("token").is_none());

    for attempt in 0..8 {
        let path = if attempt % 2 == 0 {
            "/api/auth/login/native"
        } else {
            "/api/auth/login/password"
        };
        assert_eq!(
            app.clone()
                .oneshot(login(path, "wrong-password"))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    for path in ["/api/auth/login/native", "/api/auth/login/password"] {
        let response = app
            .clone()
            .oneshot(login(path, "strong-test-password"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(response.headers().contains_key(header::RETRY_AFTER));
    }
    drop(app);
    drop(store);
    std::fs::remove_dir_all(directory).unwrap();
}
