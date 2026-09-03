use axum::{Extension, Json, extract::Path, http::{HeaderMap, StatusCode}};
use daily_mirror_vision_contract::CompletePhotoRequest;

use crate::{processing::ProcessingQueue, processor_auth};

pub async fn post(
    Extension(queue): Extension<ProcessingQueue>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<CompletePhotoRequest>,
) -> StatusCode {
    if let Err(status) = processor_auth::authorize(&headers) {
        return status;
    }
    queue
        .complete(
            &id,
            &request.pipeline_version,
            &request.lease_token,
            &request.result,
        )
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .unwrap_or_else(|error| error.status_code())
}
