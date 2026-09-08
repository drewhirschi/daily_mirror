//! Disposable, log-only preview diagnostic. Requires Vercel Deployment Protection.
//! No database, photo storage, credentials, or production application routes.
use axum::{Json, Router, http::StatusCode, routing::{get, post}};
use nextrs::WaitUntil;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tower::ServiceBuilder;

#[derive(Deserialize)]
struct Probe {
    run_id: String,
}

async fn probe(wait: WaitUntil, Json(input): Json<Probe>) -> Result<Json<Value>, StatusCode> {
    if input.run_id.is_empty() || input.run_id.len() > 64
        || !input.run_id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let run_id = input.run_id;
    let started = Instant::now();
    let label = option_env!("PROBE_RUNTIME_LABEL").unwrap_or("stock-2.4.0");
    println!("{}", json!({"probe": &run_id, "event": "scheduled", "runtime": label}));
    let background_id = run_id.clone();
    wait.wait_until(async move {
        for tick in 1..=30 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            println!("{}", json!({"probe": &background_id, "event": "tick", "tick": tick,
                "elapsed_ms": started.elapsed().as_millis(), "runtime": label}));
        }
        println!("{}", json!({"probe": background_id, "event": "complete",
            "elapsed_ms": started.elapsed().as_millis(), "runtime": label}));
    });
    Ok(Json(json!({"scheduled": true, "run_id": run_id, "ticks": 30, "runtime": label})))
}

async fn health() -> Json<Value> {
    #[cfg(target_os = "linux")]
    let libc_version = {
        unsafe extern "C" { fn gnu_get_libc_version() -> *const std::ffi::c_char; }
        // glibc returns a static, NUL-terminated version string.
        unsafe { std::ffi::CStr::from_ptr(gnu_get_libc_version()) }.to_string_lossy().into_owned()
    };
    #[cfg(not(target_os = "linux"))]
    let libc_version = "not-linux".to_owned();
    Json(json!({"ok": true, "glibc": libc_version}))
}

#[tokio::main]
async fn main() -> Result<(), vercel_runtime::Error> {
    let router = Router::new().route("/probe", post(probe)).route("/healthz", get(health));
    let service = ServiceBuilder::new()
        .layer(nextrs::vercel::StreamingVercelLayer::new())
        .service(router);
    vercel_runtime::run(service).await
}
