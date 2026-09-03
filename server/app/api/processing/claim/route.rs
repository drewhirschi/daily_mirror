use axum::{Extension, Json, http::{HeaderMap, StatusCode}};
use daily_mirror_vision_contract::{ClaimRequest, ClaimResponse};

use crate::{processing::ProcessingQueue, processor_auth};

pub async fn post(
    Extension(queue): Extension<ProcessingQueue>,
    headers: HeaderMap,
    Json(request): Json<ClaimRequest>,
) -> Result<Json<ClaimResponse>, StatusCode> {
    processor_auth::authorize(&headers)?;
    queue
        .reconcile_missing(&request.pipeline_version)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let photos = queue
        .claim(&request)
        .await
        .map_err(|error| error.status_code())?;
    Ok(Json(ClaimResponse { photos }))
}
