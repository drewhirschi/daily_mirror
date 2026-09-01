use axum::{Extension, extract::Path, http::{HeaderMap, StatusCode}};

use crate::{
    catalog::PhotoCatalog,
    photos::PhotoStore,
    realtime::RealtimeHub,
    upload_auth,
    upload_flow::finalize_upload,
};

pub async fn post(
    Extension(store): Extension<PhotoStore>,
    Extension(catalog): Extension<PhotoCatalog>,
    Extension(realtime): Extension<RealtimeHub>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> StatusCode {
    if let Err(status) = upload_auth::authorize(&headers) {
        return status;
    }
    match finalize_upload(&store, &catalog, &id).await {
        Ok(()) => {
            realtime.publish_photo("photo.created", &id).await;
            StatusCode::NO_CONTENT
        }
        Err(error) => error.status_code(),
    }
}
