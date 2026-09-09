use axum::{Extension, extract::Path, http::{HeaderMap, StatusCode}};

use crate::{
    catalog::PhotoCatalog,
    photos::PhotoStore,
    processing::ProcessingQueue,
    upload_auth,
    upload_flow::finalize_upload,
};

pub async fn post(
    Extension(store): Extension<PhotoStore>,
    Extension(catalog): Extension<PhotoCatalog>,
    Extension(processing): Extension<ProcessingQueue>,
    Path(id): Path<String>,
    wait: nextrs::WaitUntil,
    headers: HeaderMap,
) -> StatusCode {
    if let Err(status) = upload_auth::authorize(&headers) {
        return status;
    }
    finalize_upload(&store, &catalog, &processing, &id)
        .await
        .map(|()| { crate::background::notify(&wait, Some(id.clone())); StatusCode::NO_CONTENT })
        .unwrap_or_else(|error| error.status_code())
}
