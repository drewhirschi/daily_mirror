//! End-to-end cover for the capture metadata a camera sends with its upload
//! grant, and for the capture-ID rules that protect `photos.captured_at`.
//!
//! The field list and units are documented in `docs/capture-metadata.md`,
//! which is the contract the firmware implements against.

use axum::{
    Extension,
    body::Body,
    http::{Request, StatusCode, header},
    middleware,
};
use server::{
    auth::AuthStore, catalog::PhotoCatalog, devices::DeviceRegistry, passkeys::PasskeyService,
    photos::PhotoStore, processing::ProcessingQueue, view_auth,
};
use tower::ServiceExt;

const SHARED_TOKEN: &str = "capture-metadata-shared-upload-token";

struct Fixture {
    directory: std::path::PathBuf,
    app: axum::Router,
    catalog: PhotoCatalog,
}

async fn fixture() -> Fixture {
    let directory =
        std::env::temp_dir().join(format!("daily-mirror-capture-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    unsafe { std::env::set_var("DAILY_MIRROR_UPLOAD_TOKEN", SHARED_TOKEN) };
    let database = directory.join("capture.db").to_string_lossy().into_owned();
    let auth = AuthStore::local(database.clone());
    let catalog = PhotoCatalog::local(database);
    let registry = DeviceRegistry::new(ProcessingQueue::new(catalog.clone()));
    let app = nextrs::router::build_router(server::generated_registry())
        .layer(Extension(PhotoStore::new(directory.join("photos"))))
        .layer(Extension(catalog.clone()))
        .layer(Extension(ProcessingQueue::new(catalog.clone())))
        .layer(Extension(registry))
        .layer(Extension(PasskeyService::from_env().unwrap()))
        .layer(Extension(auth.clone()))
        .layer(middleware::from_fn_with_state(auth, view_auth::protect));
    Fixture {
        directory,
        app,
        catalog,
    }
}

fn grant(body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/uploads")
        .header(header::HOST, "mirror.example")
        .header(header::AUTHORIZATION, format!("Bearer {SHARED_TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn request(capture_id: &str, capture: serde_json::Value) -> serde_json::Value {
    let mut body = serde_json::json!({
        "capture_id": capture_id,
        "content_type": "image/jpeg",
        "content_length": 4096_u64,
    });
    if !capture.is_null() {
        body["capture"] = capture;
    }
    body
}

#[tokio::test]
async fn a_grant_records_what_the_camera_reported_and_refuses_what_it_cannot_have_meant() {
    let fixture = fixture().await;
    let app = &fixture.app;

    let id = "20260915T120000Z-0abcdef0";
    let accepted = app
        .clone()
        .oneshot(grant(request(
            id,
            serde_json::json!({
                "firmware_version": "0.4.1",
                "sensor": "IMX519",
                "width": 4656,
                "height": 3496,
                "jpeg_quality": 90,
                "exposure_us": 19994,
                "analog_gain": 8.0,
                "digital_gain": 1.02,
                "af_state": "focused",
                "lens_position": 3.5,
                "colour_temperature_k": 2800,
                "mean_luma": 31,
                "focus_score": 412,
                "trigger": "button",
                "captured_at": "2026-09-15T12:00:01Z",
                // A field this build has never heard of must be ignored, so
                // firmware and server can ship in either order.
                "some_future_reading": 7,
            }),
        )))
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::OK);

    let stored = fixture
        .catalog
        .capture_for_photo(id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.sensor.as_deref(), Some("imx519"));
    assert_eq!(stored.firmware_version.as_deref(), Some("0.4.1"));
    assert_eq!(stored.mean_luma, Some(31));
    assert_eq!(stored.exposure_us, Some(19_994));
    assert_eq!(stored.analog_gain, Some(8.0));
    assert_eq!(stored.af_state.as_deref(), Some("focused"));
    assert_eq!(stored.trigger.as_deref(), Some("button"));
    // The shared Pi token predates pairing, so its photographs are `legacy`.
    assert_eq!(stored.capture_source.as_deref(), Some("legacy"));

    // An absurd reading is a camera bug worth seeing, not a value to store.
    let absurd = app
        .clone()
        .oneshot(grant(request(
            "20260915T120100Z-0abcdef1",
            serde_json::json!({ "mean_luma": 9000 }),
        )))
        .await
        .unwrap();
    assert_eq!(absurd.status(), StatusCode::BAD_REQUEST);

    // A photograph with no metadata at all is still perfectly acceptable.
    let bare = app
        .clone()
        .oneshot(grant(request(
            "20260915T120200Z-0abcdef2",
            serde_json::Value::Null,
        )))
        .await
        .unwrap();
    assert_eq!(bare.status(), StatusCode::OK);

    unsafe { std::env::remove_var("DAILY_MIRROR_UPLOAD_TOKEN") };
    let _ = std::fs::remove_dir_all(fixture.directory);
}

#[tokio::test]
async fn the_pi_sidecar_shape_is_accepted_so_the_device_can_forward_it_unchanged() {
    let fixture = fixture().await;
    let id = "20260915T130000Z-0abcdef0";
    let response = fixture
        .app
        .clone()
        .oneshot(grant(serde_json::json!({
            "capture_id": id,
            "content_type": "image/jpeg",
            "content_length": 4096_u64,
            "capture": { "sensor": "imx519", "capture_source": "device" },
            "sensor_metadata": {
                "ExposureTime": 19994,
                "AnalogueGain": 8.0,
                "DigitalGain": 1.02,
                "ColourTemperature": 2800,
                "LensPosition": 3.5,
                "AfState": 2,
                "SensorTimestamp": 123456789_i64
            }
        })))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let stored = fixture
        .catalog
        .capture_for_photo(id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.exposure_us, Some(19_994));
    assert_eq!(stored.analog_gain, Some(8.0));
    assert_eq!(stored.colour_temperature_k, Some(2800));
    assert_eq!(stored.lens_position, Some(3.5));
    assert_eq!(stored.af_state.as_deref(), Some("focused"));
    assert_eq!(stored.capture_source.as_deref(), Some("device"));

    unsafe { std::env::remove_var("DAILY_MIRROR_UPLOAD_TOKEN") };
    let _ = std::fs::remove_dir_all(fixture.directory);
}

/// The catalog derives `captured_at` by slicing the capture ID, so an ID in
/// any other shape stored a timestamp that sorted above every real photograph
/// — and two cameras sharing an ID could overwrite one another's rows.
#[tokio::test]
async fn malformed_capture_ids_are_refused_at_the_grant() {
    let fixture = fixture().await;
    for junk in [
        // What the ESP32 firmware used to send.
        "esp32-p4-000001-12345678",
        "mirror-00ab4f2a-9912",
        // Right shape, unsynchronised clock.
        "19700101T000000Z-0abcdef0",
        // Right shape, wrong suffix alphabet.
        "20260915T120000Z-capture1",
        "20260915T120000Z-0ABCDEF0",
    ] {
        let response = fixture
            .app
            .clone()
            .oneshot(grant(request(junk, serde_json::Value::Null)))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "expected {junk} to be refused"
        );
    }
    unsafe { std::env::remove_var("DAILY_MIRROR_UPLOAD_TOKEN") };
    let _ = std::fs::remove_dir_all(fixture.directory);
}

#[tokio::test]
async fn a_finished_photograph_is_not_overwritten_but_its_own_retry_still_works() {
    let fixture = fixture().await;
    let id = "20260915T140000Z-0abcdef0";
    let app = &fixture.app;

    assert_eq!(
        app.clone()
            .oneshot(grant(request(id, serde_json::Value::Null)))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    fixture.catalog.mark_ready(id).await.unwrap();

    // The same camera retrying the same upload is idempotent.
    assert_eq!(
        app.clone()
            .oneshot(grant(request(id, serde_json::Value::Null)))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );

    // A different photograph reusing the ID is refused rather than silently
    // repointing the finished row at another object.
    let colliding = app
        .clone()
        .oneshot(grant(serde_json::json!({
            "capture_id": id,
            "content_type": "image/jpeg",
            "content_length": 9999_u64,
        })))
        .await
        .unwrap();
    assert_eq!(colliding.status(), StatusCode::CONFLICT);

    // And the original row is untouched.
    let photos = fixture.catalog.list().await.unwrap();
    assert_eq!(photos.len(), 1);
    assert_eq!(photos[0].id, id);

    unsafe { std::env::remove_var("DAILY_MIRROR_UPLOAD_TOKEN") };
    let _ = std::fs::remove_dir_all(fixture.directory);
}
