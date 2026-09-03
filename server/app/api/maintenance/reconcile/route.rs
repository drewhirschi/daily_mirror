use axum::{Extension, Json, http::{HeaderMap, StatusCode}};

use crate::{
    catalog::PhotoCatalog,
    cron_auth,
    photos::PhotoStore,
    processing::{ProcessingQueue, active_pipeline_version},
    upload_flow::{ReconcileReport, reconcile_all},
};

#[nextrs::api]
pub async fn get(
    Extension(store): Extension<PhotoStore>,
    Extension(catalog): Extension<PhotoCatalog>,
    Extension(processing): Extension<ProcessingQueue>,
    headers: HeaderMap,
) -> Result<Json<ReconcileReport>, StatusCode> {
    cron_auth::authorize(&headers)?;
    let report = reconcile_all(&store, &catalog)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let pipeline = active_pipeline_version().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    processing.reconcile_missing(&pipeline).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(report))
}
