//! End-to-end cover for the pairing routes described in
//! `docs/device-pairing-plan.md`: mint, claim, list, and upload authentication.

use axum::{
    Extension,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
    middleware,
};
use server::{
    auth::AuthStore, catalog::PhotoCatalog, devices::DeviceRegistry, passkeys::PasskeyService,
    photos::PhotoStore, processing::ProcessingQueue, view_auth,
};
use tower::ServiceExt;

const SHARED_TOKEN: &str = "pairing-test-shared-upload-token";

fn json_request(method: &str, path: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "mirror.example")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn bearer_json(method: &str, path: &str, token: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "mirror.example")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 64 * 1024).await.unwrap()).unwrap()
}

fn upload_body(capture_id: &str) -> serde_json::Value {
    serde_json::json!({
        "capture_id": capture_id,
        "content_type": "image/jpeg",
        "content_length": 4096_u64,
    })
}

#[tokio::test]
async fn pairing_mints_claims_lists_and_authenticates_uploads() {
    let directory =
        std::env::temp_dir().join(format!("daily-mirror-pairing-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    // The shared token is still honored for the Pi rig. Set it here so the
    // legacy path is exercised rather than silently skipped.
    unsafe { std::env::set_var("DAILY_MIRROR_UPLOAD_TOKEN", SHARED_TOKEN) };

    let database = directory.join("pairing.db").to_string_lossy().into_owned();
    let auth = AuthStore::local(database.clone());
    let catalog = PhotoCatalog::local(database);
    let registry = DeviceRegistry::new(ProcessingQueue::new(catalog.clone()));
    // Signup is what puts a user in a household; membership lives in the
    // household_users join table, which pairing and onboarding both read.
    let user = server::onboarding::signup(
        &auth,
        &ProcessingQueue::new(catalog.clone()),
        &server::onboarding::SignupRequest {
            username: "pairer".to_owned(),
            display_name: "Pairer".to_owned(),
            password: "strong-test-password".to_owned(),
        },
    )
    .await
    .unwrap();
    let session = auth.create_session(&user.id).await.unwrap().token;
    // An account that never signed up has no household and cannot mint.
    let homeless = auth
        .create_user("homeless", "Homeless", "strong-test-password")
        .await
        .unwrap();
    let homeless_session = auth.create_session(&homeless.id).await.unwrap().token;

    let app = nextrs::router::build_router(server::generated_registry())
        .layer(Extension(PhotoStore::new(directory.join("photos"))))
        .layer(Extension(catalog.clone()))
        .layer(Extension(ProcessingQueue::new(catalog.clone())))
        .layer(Extension(registry.clone()))
        .layer(Extension(PasskeyService::from_env().unwrap()))
        .layer(Extension(auth.clone()))
        .layer(middleware::from_fn_with_state(
            auth.clone(),
            view_auth::protect,
        ));

    // Minting a claim token requires a signed-in user.
    let anonymous = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/devices/claim-tokens",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    let unhoused = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            "/api/devices/claim-tokens",
            &homeless_session,
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(unhoused.status(), StatusCode::NOT_FOUND);

    let minted = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            "/api/devices/claim-tokens",
            &session,
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(minted.status(), StatusCode::OK);
    let grant = body_json(minted).await;
    // The device stores this origin, so it must be the public one.
    assert_eq!(grant["server_url"], "https://mirror.example");
    let claim_token = grant["claim_token"].as_str().unwrap().to_owned();

    // The device claims without any session: the claim token is the credential.
    let claim = |token: &str, device_id: &str| {
        json_request(
            "POST",
            "/api/devices/claim",
            serde_json::json!({
                "device_id": device_id,
                "claim_token": token,
                "firmware_version": "0.1.0",
                "hardware": "esp32-p4-imx519",
            }),
        )
    };
    let claimed = app
        .clone()
        .oneshot(claim(&claim_token, "mirror-00ab4f2a"))
        .await
        .unwrap();
    assert_eq!(claimed.status(), StatusCode::OK);
    let claimed = body_json(claimed).await;
    assert_eq!(claimed["device_name"], "Mirror 4F2A");
    let device_token = claimed["device_token"].as_str().unwrap().to_owned();

    // Single use.
    assert_eq!(
        app.clone()
            .oneshot(claim(&claim_token, "mirror-00ab4f2a"))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );

    // The household's device list needs a session and shows the new device.
    assert_eq!(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/devices")
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let listed = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/devices")
                .header(header::AUTHORIZATION, format!("Bearer {session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    let listed = body_json(listed).await;
    assert_eq!(listed["devices"].as_array().unwrap().len(), 1);
    assert_eq!(listed["devices"][0]["device_id"], "mirror-00ab4f2a");
    assert!(listed["devices"][0]["last_seen_at"].is_null());

    // The per-device token authorizes an upload and stamps the photo row.
    let device_capture = "20260915T120000Z-devicea1";
    let upload = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            "/api/uploads",
            &device_token,
            upload_body(device_capture),
        ))
        .await
        .unwrap();
    assert_eq!(upload.status(), StatusCode::OK);
    assert_eq!(
        catalog.device_for_photo(device_capture).await.unwrap(),
        Some("mirror-00ab4f2a".to_owned())
    );

    // Presence is recorded on use.
    let listed = body_json(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/devices")
                    .header(header::AUTHORIZATION, format!("Bearer {session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert!(listed["devices"][0]["last_seen_at"].is_string());

    // The completion route accepts the device token too. There is no object in
    // storage, so it fails past authentication rather than at it.
    let completed = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            &format!("/api/uploads/{device_capture}"),
            &device_token,
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    assert_ne!(completed.status(), StatusCode::UNAUTHORIZED);

    // TODO(pairing): drop with the shared token. The Pi rig still uses it.
    let shared_capture = "20260915T120500Z-shared01";
    let shared = app
        .clone()
        .oneshot(bearer_json(
            "POST",
            "/api/uploads",
            SHARED_TOKEN,
            upload_body(shared_capture),
        ))
        .await
        .unwrap();
    assert_eq!(shared.status(), StatusCode::OK);
    assert_eq!(
        catalog.device_for_photo(shared_capture).await.unwrap(),
        None
    );

    // Anything else is rejected.
    assert_eq!(
        app.clone()
            .oneshot(bearer_json(
                "POST",
                "/api/uploads",
                &"n".repeat(43),
                upload_body("20260915T121000Z-nobody01"),
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );

    drop(app);
    unsafe { std::env::remove_var("DAILY_MIRROR_UPLOAD_TOKEN") };
    std::fs::remove_dir_all(directory).unwrap();
}
