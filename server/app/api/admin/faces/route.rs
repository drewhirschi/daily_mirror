use axum::{Extension, Json, http::StatusCode};

use crate::{
    face_admin::{AdminFaceDashboard, BatchAssignFacesRequest, BatchAssignFacesResponse},
    processing::ProcessingQueue,
};

#[nextrs::api]
pub async fn get(
    Extension(queue): Extension<ProcessingQueue>,
) -> Result<Json<AdminFaceDashboard>, StatusCode> {
    queue
        .admin_dashboard()
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[nextrs::api]
pub async fn patch(
    Extension(queue): Extension<ProcessingQueue>,
    Json(request): Json<BatchAssignFacesRequest>,
) -> Result<Json<BatchAssignFacesResponse>, StatusCode> {
    queue
        .assign_faces(&request.assignments)
        .await
        .map(Json)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
            std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        })
}
