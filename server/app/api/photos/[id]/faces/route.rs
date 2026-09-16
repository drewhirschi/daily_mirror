use axum::{Extension, Json, extract::Path, http::StatusCode};

use crate::{face_admin::PhotoFacesResponse, processing::ProcessingQueue};

/// Face bounds and identities for one photograph, so viewers can label who
/// the pipeline found without loading the admin dashboard.
#[nextrs::api]
pub async fn get(
    Path(id): Path<String>,
    Extension(queue): Extension<ProcessingQueue>,
) -> Result<Json<PhotoFacesResponse>, StatusCode> {
    queue
        .photo_faces(&id)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
