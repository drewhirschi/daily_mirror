use axum::{Extension, Json, extract::Path, http::StatusCode};

use crate::{
    face_admin::{HouseholdConfig, UpdateHouseholdRequest},
    processing::ProcessingQueue,
};

#[nextrs::api]
pub async fn patch(
    Path(id): Path<String>,
    Extension(queue): Extension<ProcessingQueue>,
    Json(request): Json<UpdateHouseholdRequest>,
) -> Result<Json<HouseholdConfig>, StatusCode> {
    queue
        .update_household(&id, &request)
        .await
        .map(Json)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
            std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        })
}
