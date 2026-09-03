use axum::{Extension, Json, extract::Path, http::{HeaderMap, StatusCode}};
use daily_mirror_vision_contract::LeaseRequest;

use crate::{processing::ProcessingQueue, processor_auth};

pub async fn post(
    Extension(queue): Extension<ProcessingQueue>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<LeaseRequest>,
) -> StatusCode {
    if let Err(status) = processor_auth::authorize(&headers) {
        return status;
    }
    queue
        .renew(&id, &request.pipeline_version, &request.lease_token)
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .unwrap_or_else(|error| error.status_code())
}
