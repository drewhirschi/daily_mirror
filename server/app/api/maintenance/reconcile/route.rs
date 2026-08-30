use axum::{Extension, Json, http::{HeaderMap, StatusCode}};

use crate::{
    catalog::PhotoCatalog,
    cron_auth,
    photos::PhotoStore,
    upload_flow::{ReconcileReport, reconcile_all},
};

#[nextrs::api]
pub async fn get(
    Extension(store): Extension<PhotoStore>,
    Extension(catalog): Extension<PhotoCatalog>,
    headers: HeaderMap,
) -> Result<Json<ReconcileReport>, StatusCode> {
    cron_auth::authorize(&headers)?;
    let report = reconcile_all(&store, &catalog)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(report))
}
