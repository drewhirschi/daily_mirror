use axum::{Extension, Json};

use crate::auth::User;

#[nextrs::api]
pub async fn get(Extension(user): Extension<User>) -> Json<User> {
    Json(user)
}
