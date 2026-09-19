use axum::{Extension, Json, extract::Path, response::Response};

use crate::{
    auth::User,
    catalog::PhotoCatalog,
    onboarding,
    photos::PhotoStore,
    processing::ProcessingQueue,
    upload_flow::{UploadGrant, UploadRequest},
};

/// Session authenticated: the caller's own household decides who may be
/// enrolled, so the device upload token plays no part here.
#[nextrs::api(responses((status = 200, body = UploadGrant)))]
pub async fn post(
    Path(person_id): Path<String>,
    Extension(store): Extension<PhotoStore>,
    Extension(catalog): Extension<PhotoCatalog>,
    Extension(queue): Extension<ProcessingQueue>,
    Extension(user): Extension<User>,
    Json(request): Json<UploadRequest>,
) -> Result<Json<UploadGrant>, Response> {
    onboarding::create_enrollment_upload(&store, &catalog, &queue, &user, &person_id, &request)
        .await
        .map(Json)
        .map_err(onboarding::http_error)
}
