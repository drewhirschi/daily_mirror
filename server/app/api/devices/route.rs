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
        .map_err(household_status)?;
    registry
        .list(&household_id)
        .await
        .map(|devices| Json(DeviceListResponse { devices }))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// A signed-in account with no household row is a 404, not a server fault:
/// there is no invite flow yet, so membership comes from signup or from an
/// administrator.
fn household_status(error: std::io::Error) -> StatusCode {
    match error.kind() {
        std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
