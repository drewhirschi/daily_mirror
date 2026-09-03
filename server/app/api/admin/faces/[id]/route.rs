use axum::{Extension, Json, extract::Path, http::StatusCode};

use crate::{
    face_admin::{AssignFaceRequest, AssignFaceResponse},
    processing::ProcessingQueue,
};

#[nextrs::api]
pub async fn patch(
    Extension(queue): Extension<ProcessingQueue>,
    Path(id): Path<String>,
    Json(request): Json<AssignFaceRequest>,
) -> Result<Json<AssignFaceResponse>, StatusCode> {
    queue
        .assign_face(&id, request.person_id.as_deref())
        .await
        .map(Json)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
            std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        })
}
