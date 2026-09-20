use axum::{Extension, Json, response::Response};

use crate::{
    auth::{AuthStore, User},
    onboarding::{self, HouseholdSummary, RenameHouseholdRequest},
    processing::ProcessingQueue,
};

#[nextrs::api(responses((status = 200, body = HouseholdSummary)))]
pub async fn get(
    Extension(queue): Extension<ProcessingQueue>,
    Extension(auth): Extension<AuthStore>,
    Extension(user): Extension<User>,
) -> Result<Json<HouseholdSummary>, Response> {
    onboarding::household_for_user(&queue, &auth, &user)
        .await
        .map(Json)
        .map_err(onboarding::http_error)
}

/// Renaming is an administrator action; members get 403.
#[nextrs::api(responses((status = 200, body = HouseholdSummary)))]
pub async fn patch(
    Extension(queue): Extension<ProcessingQueue>,
    Extension(auth): Extension<AuthStore>,
    Extension(user): Extension<User>,
    Json(request): Json<RenameHouseholdRequest>,
) -> Result<Json<HouseholdSummary>, Response> {
    onboarding::rename_household(&queue, &auth, &user, &request.display_name)
        .await
        .map(Json)
        .map_err(onboarding::http_error)
}
