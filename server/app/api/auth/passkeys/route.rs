use axum::{Extension, Json, http::StatusCode};
use serde::Serialize;

use crate::auth::{AuthStore, PasskeySummary, User};

#[derive(Serialize)]
pub struct PasskeyList {
    passkeys: Vec<PasskeySummary>,
}

pub async fn get(
    Extension(store): Extension<AuthStore>,
    Extension(user): Extension<User>,
) -> Result<Json<PasskeyList>, StatusCode> {
    let passkeys = store
        .passkey_summaries(&user.id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(PasskeyList { passkeys }))
}
