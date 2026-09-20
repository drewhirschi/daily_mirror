use axum::{Extension, extract::Path, http::{HeaderMap, StatusCode}};

use crate::{
    catalog::PhotoCatalog,
    devices::DeviceRegistry,
    photos::PhotoStore,
    processing::ProcessingQueue,
    upload_auth,
    upload_flow::finalize_upload,
};

pub async fn post(
    Extension(store): Extension<PhotoStore>,
    Extension(catalog): Extension<PhotoCatalog>,
    Extension(processing): Extension<ProcessingQueue>,
    Extension(registry): Extension<DeviceRegistry>,
    Path(id): Path<String>,
    wait: nextrs::WaitUntil,
    headers: HeaderMap,
) -> StatusCode {
    if let Err(status) = upload_auth::authorize(&registry, &headers).await {
        return status;
    }
    match finalize_upload(&store, &catalog, &processing, &id).await {
        Ok(()) => {
            crate::background::notify(&wait, Some(id.clone()));
            StatusCode::NO_CONTENT
        }
        Err(error) => {
            // A bare status in the camera's log is not enough to debug from:
            // say what actually went wrong. Nothing here carries a credential.
            let status = error.status_code();
            eprintln!("upload_finalize_failed capture_id={id} status={status} error={error}");
            status
        }
    }
}
