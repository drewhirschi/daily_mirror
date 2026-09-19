use axum::{Extension, Json, extract::Path, response::Response};

use crate::{
    auth::User,
    onboarding::{self, EnrollmentStatus},
    processing::ProcessingQueue,
};

#[nextrs::api(responses((status = 200, body = EnrollmentStatus)))]
pub async fn get(
    Path(person_id): Path<String>,
    Extension(queue): Extension<ProcessingQueue>,
    Extension(user): Extension<User>,
) -> Result<Json<EnrollmentStatus>, Response> {
    onboarding::enrollment_status(&queue, &user, &person_id)
        .await
        .map(Json)
        .map_err(onboarding::http_error)
}
