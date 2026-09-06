use axum::{Extension, Json, http::StatusCode};
use crate::{face_matching::MatchingSummary, processing::ProcessingQueue};

#[nextrs::api]
pub async fn post(Extension(queue): Extension<ProcessingQueue>) -> Result<Json<MatchingSummary>, StatusCode> {
    queue.refresh_face_suggestions().await.map(Json).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
