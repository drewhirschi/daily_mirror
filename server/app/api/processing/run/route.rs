use crate::{
    background::{self, RunRequest},
    photos::PhotoStore,
    processing::ProcessingQueue,
    processor_auth,
};
use axum::{
    Extension, Json,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use nextrs::WaitUntil;

pub async fn post(
    Extension(queue): Extension<ProcessingQueue>,
    Extension(store): Extension<PhotoStore>,
    wait: WaitUntil,
    headers: HeaderMap,
    Json(request): Json<RunRequest>,
) -> Response {
    if let Err(status) = processor_auth::authorize(&headers) {
        return status.into_response();
    }
    if !background::enabled() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    #[cfg(feature = "face-inference")]
    {
        match crate::vision::run(queue, store, wait, request).await {
            Ok(report) => Json(report).into_response(),
            Err(status) => status.into_response(),
        }
    }
    #[cfg(not(feature = "face-inference"))]
    {
        let _ = (queue, store, wait, request);
        StatusCode::SERVICE_UNAVAILABLE.into_response()
    }
}
