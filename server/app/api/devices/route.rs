use axum::{Extension, Json, http::StatusCode};
use daily_mirror_core::contract::DeviceListResponse;

use crate::{auth::User, devices::DeviceRegistry};

/// The caller's household devices. Session authenticated by `view_auth`.
#[nextrs::api]
pub async fn get(
    Extension(registry): Extension<DeviceRegistry>,
    Extension(user): Extension<User>,
) -> Result<Json<DeviceListResponse>, StatusCode> {
    let household_id = registry
        .household_for_user(&user)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    registry
        .list(&household_id)
        .await
        .map(|devices| Json(DeviceListResponse { devices }))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
