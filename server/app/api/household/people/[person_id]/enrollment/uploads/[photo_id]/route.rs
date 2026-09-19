use axum::{Extension, extract::Path, http::StatusCode, response::Response};

use crate::{
    auth::User, catalog::PhotoCatalog, onboarding, photos::PhotoStore,
    processing::ProcessingQueue,
};

#[nextrs::api(responses((status = 204, description = "The enrollment photo was accepted for processing")))]
pub async fn post(
    Path((person_id, photo_id)): Path<(String, String)>,
    Extension(store): Extension<PhotoStore>,
    Extension(catalog): Extension<PhotoCatalog>,
    Extension(queue): Extension<ProcessingQueue>,
    Extension(user): Extension<User>,
    wait: nextrs::WaitUntil,
) -> Result<StatusCode, Response> {
    onboarding::finalize_enrollment_upload(&store, &catalog, &queue, &user, &person_id, &photo_id)
        .await
        .map(|()| {
            crate::background::notify(&wait, Some(photo_id.clone()));
            StatusCode::NO_CONTENT
        })
        .map_err(onboarding::http_error)
}
