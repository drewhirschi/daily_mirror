use axum::{Extension, Json, extract::Path, http::StatusCode};

use crate::{face_admin::PersonPhotosResponse, processing::ProcessingQueue};

#[nextrs::api]
pub async fn get(
    Path(id): Path<String>,
    Extension(queue): Extension<ProcessingQueue>,
) -> Result<Json<PersonPhotosResponse>, StatusCode> {
    queue.person_photos(&id).await.map(Json).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
