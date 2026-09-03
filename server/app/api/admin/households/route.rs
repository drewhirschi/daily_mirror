use axum::{Extension, Json, http::StatusCode};

use crate::{
    face_admin::{CreateHouseholdRequest, HouseholdConfig, HouseholdsResponse},
    processing::ProcessingQueue,
};

#[nextrs::api]
pub async fn get(
    Extension(queue): Extension<ProcessingQueue>,
) -> Result<Json<HouseholdsResponse>, StatusCode> {
    queue
        .households()
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[nextrs::api]
pub async fn post(
    Extension(queue): Extension<ProcessingQueue>,
    Json(request): Json<CreateHouseholdRequest>,
) -> Result<Json<HouseholdConfig>, StatusCode> {
    queue
        .create_household(&request.display_name, request.grid_size)
        .await
        .map(Json)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        })
}
