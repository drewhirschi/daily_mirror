use axum::{Extension, Json};

use crate::auth::User;

pub async fn get(Extension(user): Extension<User>) -> Json<User> {
    Json(user)
}
