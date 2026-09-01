use axum::{Extension, Json, http::{HeaderMap, StatusCode}};

use crate::{
    catalog::PhotoCatalog,
    cron_auth,
    photos::PhotoStore,
    realtime::RealtimeHub,
    upload_flow::{ReconcileReport, reconcile_all},
};

#[nextrs::api]
pub async fn get(
    Extension(store): Extension<PhotoStore>,
    Extension(catalog): Extension<PhotoCatalog>,
    Extension(realtime): Extension<RealtimeHub>,
    headers: HeaderMap,
) -> Result<Json<ReconcileReport>, StatusCode> {
    cron_auth::authorize(&headers)?;
    let report = reconcile_all(&store, &catalog)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if report.ready_after != report.ready_before || report.generated_thumbnails > 0 {
        realtime.publish_reconciled().await;
    }
    Ok(Json(report))
}
