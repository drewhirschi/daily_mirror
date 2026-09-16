use axum::{Extension, Json, http::StatusCode};
use daily_mirror_core::contract::{DeviceClaimRequest, DeviceClaimed};

use crate::devices::DeviceRegistry;

/// Redeem a claim token for a per-device bearer token.
///
/// Deliberately unauthenticated: the claim token is the credential, and the
/// device has no session. 409 means the device already belongs to another
/// household and was not released by a full reset.
#[nextrs::api]
pub async fn post(
    Extension(registry): Extension<DeviceRegistry>,
    Json(request): Json<DeviceClaimRequest>,
) -> Result<Json<DeviceClaimed>, StatusCode> {
    registry
        .claim(&request)
        .await
        .map(Json)
        .map_err(|error| error.status_code())
}
