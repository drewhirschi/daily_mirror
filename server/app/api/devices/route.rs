use axum::{Extension, Json, response::Response};
use daily_mirror_core::contract::DeviceListResponse;

use crate::{auth::User, auth_http, devices::DeviceRegistry};

/// The caller's household devices. Session authenticated by `view_auth`.
#[nextrs::api]
pub async fn get(
    Extension(registry): Extension<DeviceRegistry>,
    Extension(user): Extension<User>,
) -> Result<Json<DeviceListResponse>, Response> {
    let household_id = crate::devices::require_household(&registry, &user).await?;
    registry
        .list(&household_id)
        .await
        .map(|devices| Json(DeviceListResponse { devices }))
        .map_err(|_| {
            auth_http::error(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Device service unavailable",
            )
        })
}
