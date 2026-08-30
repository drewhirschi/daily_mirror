//! Shared Rust application construction.
//!
//! Put domain modules beside this file or below `src/`. Both the local
//! process (`main.rs`) and deployment adapters (`api/index.rs`) call `app()`,
//! so application behavior is defined once.

use axum::{Extension, extract::DefaultBodyLimit, middleware};

pub mod catalog;
pub mod photos;
pub mod upload_auth;
pub mod view_auth;

include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

pub fn app() -> axum::Router {
    let public_dir = std::env::var("NEXTRS_PUBLIC_DIR")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/public").to_string());

    let photo_store = photos::PhotoStore::from_env()
        .unwrap_or_else(|error| panic!("invalid photo storage configuration: {error}"));
    let photo_catalog = catalog::PhotoCatalog::from_env()
        .unwrap_or_else(|error| panic!("invalid photo catalog configuration: {error}"));
    upload_auth::validate_configuration(&photo_store)
        .unwrap_or_else(|error| panic!("invalid upload authentication configuration: {error}"));
    view_auth::validate_configuration(&photo_store)
        .unwrap_or_else(|error| panic!("invalid gallery authentication configuration: {error}"));
    nextrs::router::build_router_with_public(generated_registry(), &public_dir)
        .merge(nextrs::openapi::spec_router(generated_openapi()))
        .layer(DefaultBodyLimit::max(32 * 1024 * 1024))
        .layer(Extension(photo_store))
        .layer(Extension(photo_catalog))
        .layer(middleware::from_fn(view_auth::protect))
}
