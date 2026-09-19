use axum::{Extension, Json, response::Response};

use crate::{
    auth::User,
    onboarding::{self, HouseholdSummary},
    processing::ProcessingQueue,
};

#[nextrs::api(responses((status = 200, body = HouseholdSummary)))]
pub async fn get(
    Extension(queue): Extension<ProcessingQueue>,
    Extension(user): Extension<User>,
) -> Result<Json<HouseholdSummary>, Response> {
    onboarding::household_for_user(&queue, &user)
        .await
        .map(Json)
        .map_err(onboarding::http_error)
}
