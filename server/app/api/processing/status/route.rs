use axum::{Extension, Json, http::{HeaderMap, StatusCode}};
use daily_mirror_vision_contract::{QueueStatus, QueueStatusRequest};

use crate::{processing::ProcessingQueue, processor_auth};

pub async fn post(
    Extension(queue): Extension<ProcessingQueue>,
    headers: HeaderMap,
    Json(request): Json<QueueStatusRequest>,
) -> Result<Json<QueueStatus>, StatusCode> {
    processor_auth::authorize(&headers)?;
    queue
        .reconcile_missing(&request.pipeline_version)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    queue
        .status(&request.pipeline_version)
        .await
        .map(Json)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        })
}
