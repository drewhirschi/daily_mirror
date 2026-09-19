//! Signup goes through the real router so its gate, rate limiter and session
//! response are exercised together.
use axum::{
    Extension,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
    middleware,
};
use server::{
    auth::AuthStore, catalog::PhotoCatalog, passkeys::PasskeyService, processing::ProcessingQueue,
    view_auth,
};
use tower::ServiceExt;

struct Fixture {
    directory: std::path::PathBuf,
    store: AuthStore,
    app: axum::Router,
}

fn fixture() -> Fixture {
    let directory =
        std::env::temp_dir().join(format!("daily-mirror-onboarding-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let store = AuthStore::local(directory.join("auth.db").to_string_lossy().into_owned());
    let catalog = PhotoCatalog::local(directory.join("catalog.db").to_string_lossy().into_owned());
    let app = nextrs::router::build_router(server::generated_registry())
        .layer(Extension(ProcessingQueue::new(catalog.clone())))
        .layer(Extension(catalog))
        .layer(Extension(store.clone()))
        .layer(Extension(PasskeyService::from_env().unwrap()))
        .layer(middleware::from_fn_with_state(
            store.clone(),
            view_auth::protect,
        ));
    Fixture {
        directory,
        store,
        app,
    }
}

fn signup_request(username: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/auth/signup")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({
                "username": username,
                "display_name": "Drew",
                "password": "a-good-test-password",
            })
            .to_string(),
        ))
        .unwrap()
}

#[tokio::test]
async fn signup_is_refused_until_it_is_explicitly_enabled() {
    // Environment mutation is process-global, so both halves of the gate live
    // in one test to keep the variable under a single owner.
    let fixture = fixture();
    unsafe { std::env::remove_var("DAILY_MIRROR_ALLOW_SIGNUP") };
    let refused = fixture
        .app
        .clone()
        .oneshot(signup_request("gated"))
        .await
        .unwrap();
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    assert!(
        fixture
            .store
            .user_by_username("gated")
            .await
            .unwrap()
            .is_none()
    );

    unsafe { std::env::set_var("DAILY_MIRROR_ALLOW_SIGNUP", "1") };
    let response = fixture
        .app
        .clone()
        .oneshot(signup_request("newcomer"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert!(response.headers().get(header::SET_COOKIE).is_none());
    let session: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
    assert_eq!(session["expires_in_seconds"], 30 * 24 * 60 * 60);
    let token = session["token"].as_str().unwrap().to_owned();
    let person_id = session["user"]["person_id"].as_str().unwrap().to_owned();
    // Membership is not on the user: it lives in the household_users join
    // table and is observed through /api/household below.
    assert!(session["user"]["household_id"].is_null());

    // The new session immediately sees its own household.
    let household = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/household")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(household.status(), StatusCode::OK);
    let summary: serde_json::Value =
        serde_json::from_slice(&to_bytes(household.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(summary["self_person_id"], person_id);
    assert_eq!(summary["people"][0]["id"], person_id);
    assert_eq!(summary["people"][0]["enrollment"]["required_photos"], 5);
    assert_eq!(summary["people"][0]["enrollment"]["enrolled"], false);

    // A repeated username is a conflict, not a second household.
    let duplicate = fixture
        .app
        .clone()
        .oneshot(signup_request("newcomer"))
        .await
        .unwrap();
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);

    // Signup without a session is public; the household routes are not.
    let anonymous = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/household")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    unsafe { std::env::remove_var("DAILY_MIRROR_ALLOW_SIGNUP") };
    drop(fixture.app);
    drop(fixture.store);
    std::fs::remove_dir_all(fixture.directory).unwrap();
}
