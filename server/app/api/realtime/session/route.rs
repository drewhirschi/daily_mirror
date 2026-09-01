use axum::{Extension, Json, http::StatusCode};

use crate::realtime::{RealtimeHub, RealtimeSession};

#[nextrs::api]
pub async fn get(
    Extension(hub): Extension<RealtimeHub>,
) -> Result<Json<RealtimeSession>, StatusCode> {
    hub.session()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}
