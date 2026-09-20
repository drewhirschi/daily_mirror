use axum::{
    Extension, Json,
    http::{HeaderMap, StatusCode},
    response::Response,
};
use daily_mirror_core::contract::ClaimTokenGrant;

use crate::{
    auth::User,
    auth_http,
    devices::{DeviceRegistry, public_origin},
};

/// Mint a single-use claim token bound to the signed-in user's household.
///
/// The app hands the token to a device over the local provisioning link; the
/// device redeems it at `POST /api/devices/claim` within
/// `CLAIM_TOKEN_TTL_SECONDS`.
///
/// An account with no household is refused with 409 rather than having one
/// guessed for it, so a camera can never be paired into a stranger's home.
#[nextrs::api]
pub async fn post(
    Extension(registry): Extension<DeviceRegistry>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
) -> Result<Json<ClaimTokenGrant>, Response> {
    // The device stores this origin and uses it for every later request, so it
    // must be the public one the app itself reached.
    let server_url = public_origin(&headers)
        .ok_or_else(|| auth_http::error(StatusCode::BAD_REQUEST, "Unknown server address"))?;
    let household_id = crate::devices::require_household(&registry, &user).await?;
    registry
        .mint_claim_token_for(&household_id, &user.id, &server_url)
        .await
        .map(Json)
        .map_err(|_| {
            auth_http::error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Device service unavailable",
            )
        })
}
