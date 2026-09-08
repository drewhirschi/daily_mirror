use axum::{
    Extension,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
    middleware,
};
use server::{auth::AuthStore, passkeys::PasskeyService, view_auth};
use tower::ServiceExt;
use webauthn_authenticator_rs::{WebauthnAuthenticator, softpasskey::SoftPasskey};
use webauthn_rs::prelude::Url;

fn post(path: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn signed_passkey_creates_native_session_and_cannot_be_replayed() {
    let directory = std::env::temp_dir().join(format!("native-passkey-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let store = AuthStore::local(directory.join("auth.db").to_string_lossy().into_owned());
    let user = store
        .create_user("passkey-test", "Passkey Test", "a-test-password-only")
        .await
        .unwrap();
    let passkeys = PasskeyService::from_env().unwrap();
    let origin = Url::parse(
        &std::env::var("DAILY_MIRROR_AUTH_ORIGIN")
            .unwrap_or_else(|_| "http://localhost:3000".into()),
    )
    .unwrap();
    // Software authenticator exists only in this test, simulating user verification.
    let mut authenticator = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let registration = passkeys.start_registration(&store, &user).await.unwrap();
    let credential = authenticator
        .do_registration(origin.clone(), registration.options)
        .unwrap();
    passkeys
        .finish_registration(
            &store,
            &user,
            &registration.ceremony_id,
            "Test",
            &credential,
        )
        .await
        .unwrap();
    let app = nextrs::router::build_router(server::generated_registry())
        .layer(Extension(store.clone()))
        .layer(Extension(passkeys))
        .layer(middleware::from_fn_with_state(
            store.clone(),
            view_auth::protect,
        ));
    let response = app
        .clone()
        .oneshot(post(
            "/api/auth/login/native/passkey/start",
            serde_json::json!({"username":"passkey-test"}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let start: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    let signed = authenticator
        .do_authentication(
            origin,
            serde_json::from_value(start["options"].clone()).unwrap(),
        )
        .unwrap();
    let finish = serde_json::json!({"ceremony_id": start["ceremony_id"], "credential": signed});
    let response = app
        .clone()
        .oneshot(post(
            "/api/auth/login/native/passkey/finish",
            finish.clone(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert!(response.headers().get(header::SET_COOKIE).is_none());
    let session: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
    assert_eq!(session["user"]["id"], user.id);
    let token = session["token"].as_str().unwrap();
    assert_eq!(
        store.authenticate_session(token).await.unwrap().unwrap().id,
        user.id
    );
    assert_eq!(session["expires_in_seconds"], 30 * 24 * 60 * 60);
    let replay = app
        .clone()
        .oneshot(post("/api/auth/login/native/passkey/finish", finish))
        .await
        .unwrap();
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(replay.headers()[header::CACHE_CONTROL], "no-store");
    let logout = Request::builder()
        .method("POST")
        .uri("/api/auth/logout")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.oneshot(logout).await.unwrap().status(),
        StatusCode::NO_CONTENT
    );
    assert!(store.authenticate_session(token).await.unwrap().is_none());
    drop(store);
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn association_is_public_and_unknown_accounts_are_rate_limited() {
    let directory =
        std::env::temp_dir().join(format!("native-passkey-limit-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let store = AuthStore::local(directory.join("auth.db").to_string_lossy().into_owned());
    let app = nextrs::router::build_router(server::generated_registry())
        .layer(Extension(store.clone()))
        .layer(Extension(PasskeyService::from_env().unwrap()))
        .layer(middleware::from_fn_with_state(
            store.clone(),
            view_auth::protect,
        ));
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/.well-known/apple-app-site-association")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let association: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
    assert_eq!(
        association["webcredentials"]["apps"][0],
        "C9P58ZP4AQ.app.dailymirror.ios"
    );
    for _ in 0..8 {
        let response = app
            .clone()
            .oneshot(post(
                "/api/auth/login/native/passkey/start",
                serde_json::json!({"username":"missing"}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    let response = app
        .oneshot(post(
            "/api/auth/login/native/passkey/start",
            serde_json::json!({"username":"missing"}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(response.headers().contains_key(header::RETRY_AFTER));
    drop(store);
    std::fs::remove_dir_all(directory).unwrap();
}
