use axum::{
    Extension, Json,
    http::{HeaderMap, StatusCode},
};
use daily_mirror_core::contract::ClaimTokenGrant;

use crate::{
    auth::User,
    devices::{DeviceRegistry, public_origin},
};

/// Mint a single-use claim token bound to the signed-in user's household.
///
/// The app hands the token to a device over the local provisioning link; the
/// device redeems it at `POST /api/devices/claim` within
/// `CLAIM_TOKEN_TTL_SECONDS`.
#[nextrs::api]
pub async fn post(
    Extension(registry): Extension<DeviceRegistry>,
    Extension(user): Extension<User>,
    headers: HeaderMap,
) -> Result<Json<ClaimTokenGrant>, StatusCode> {
    // The device stores this origin and uses it for every later request, so it
    // must be the public one the app itself reached.
    let server_url = public_origin(&headers).ok_or(StatusCode::BAD_REQUEST)?;
    registry
        .mint_claim_token(&user, &server_url)
        .await
        .map(Json)
        .map_err(|error| match error.kind() {
            // No household row means no invite has been accepted and no signup
            // has happened for this account; not a server fault.
            std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        })
}
