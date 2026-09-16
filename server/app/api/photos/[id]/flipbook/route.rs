use axum::{Extension, Json, extract::Path, http::StatusCode};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::catalog::PhotoCatalog;

/// Whether a photograph may appear in people's flipbooks. The flag lives on
/// the photo, so it survives rotation, which discards and re-detects faces.
#[derive(Deserialize, Serialize, ToSchema)]
pub struct FlipbookMembership {
    pub included: bool,
}

#[nextrs::api]
pub async fn put(
    Extension(catalog): Extension<PhotoCatalog>,
    Path(id): Path<String>,
    Json(membership): Json<FlipbookMembership>,
) -> Result<Json<FlipbookMembership>, StatusCode> {
    match catalog.set_flipbook_excluded(&id, !membership.included).await {
        Ok(true) => Ok(Json(membership)),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}
