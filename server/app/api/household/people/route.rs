use axum::{Extension, Json, response::Response};

use crate::{
    auth::User,
    face_admin::CreatePersonRequest,
    onboarding::{self, HouseholdPerson},
    processing::ProcessingQueue,
};

#[nextrs::api(responses((status = 200, body = HouseholdPerson)))]
pub async fn post(
    Extension(queue): Extension<ProcessingQueue>,
    Extension(user): Extension<User>,
    Json(request): Json<CreatePersonRequest>,
) -> Result<Json<HouseholdPerson>, Response> {
    onboarding::add_household_person(&queue, &user, &request.display_name)
        .await
        .map(Json)
        .map_err(onboarding::http_error)
}
