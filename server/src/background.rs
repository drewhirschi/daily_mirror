//! Immediate notification of durable jobs; cron recovers missed notifications.
use std::time::Duration;

use nextrs::WaitUntil;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct RunRequest {
    pub photo_id: Option<String>,
    #[serde(default = "continue_by_default")]
    pub continue_queue: bool,
}

fn continue_by_default() -> bool {
    true
}

pub fn enabled() -> bool {
    std::env::var("DAILY_MIRROR_HOSTED_PROCESSING").as_deref() == Ok("1")
}

pub fn notify(wait: &WaitUntil, photo_id: Option<String>) {
    if !enabled() {
        return;
    }
    wait.wait_until(async move {
        if let Err(error) = dispatch(photo_id).await {
            // Do not log signed object URLs or credentials. The DB job remains
            // pending/leased, and the recovery cron will try again.
            eprintln!("hosted processing dispatch failed: {error}");
        }
    });
}

pub async fn dispatch(photo_id: Option<String>) -> Result<(), String> {
    let url = std::env::var("DAILY_MIRROR_WORKER_URL")
        .map_err(|_| "worker URL is not configured".to_owned())?;
    let parsed = reqwest::Url::parse(&url).map_err(|_| "invalid worker URL")?;
    if parsed.scheme() != "https" && parsed.host_str() != Some("127.0.0.1") {
        return Err("worker URL requires HTTPS".to_owned());
    }
    let token = std::env::var("DAILY_MIRROR_PROCESSOR_TOKEN")
        .map_err(|_| "processor token is not configured".to_owned())?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|_| "create dispatch client")?;
    let body = serde_json::to_vec(&RunRequest {
        photo_id,
        continue_queue: true,
    })
    .map_err(|_| "encode dispatch request")?;
    let response = client
        .post(parsed)
        .bearer_auth(token)
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|_| "worker request failed or timed out")?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("worker returned {}", response.status()))
    }
}
