use crate::{
    background, cron_auth,
    processing::{ProcessingQueue, active_pipeline_version},
};
use axum::{
    Extension,
    http::{HeaderMap, StatusCode},
};

#[nextrs::cron(schedule = "*/5 * * * *", provider = "cloudflare")]
pub async fn get(
    Extension(queue): Extension<ProcessingQueue>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    cron_auth::authorize(&headers)?;
    if !background::enabled() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let pipeline = active_pipeline_version().map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    queue
        .reconcile_missing(&pipeline)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    background::dispatch(None)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(StatusCode::OK)
}
