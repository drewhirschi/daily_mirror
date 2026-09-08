use axum::Json;

/// Public proof that the signed iOS app may use this site's passkeys.
/// Keep this App ID in sync with mobile/app.config.ts when changing teams.
pub async fn get() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "webcredentials": { "apps": ["C9P58ZP4AQ.app.dailymirror.ios"] }
    }))
}
