use axum::{Extension, Json, http::StatusCode};

use crate::{
    face_admin::{CreatePersonRequest, CreatePersonResponse, PeopleResponse},
    processing::ProcessingQueue,
};

#[nextrs::api]
pub async fn get(
    Extension(queue): Extension<ProcessingQueue>,
) -> Result<Json<PeopleResponse>, StatusCode> {
    queue
        .people_with_flipbooks()
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[nextrs::api]
pub async fn post(
    Extension(queue): Extension<ProcessingQueue>,
    Json(request): Json<CreatePersonRequest>,
) -> Result<Json<CreatePersonResponse>, StatusCode> {
    queue
        .create_person(&request.display_name)
        .await
        .map(Json)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        })
}
