//! Shared Rust application construction.
//!
//! Put domain modules beside this file or below `src/`. Both the local
//! process (`main.rs`) and deployment adapters (`api/index.rs`) call `app()`,
//! so application behavior is defined once.

use axum::{Extension, extract::DefaultBodyLimit, middleware};

pub mod auth;
pub mod auth_http;
pub mod catalog;
pub mod cron_auth;
pub mod face_admin;
pub mod passkeys;
pub mod photos;
pub mod processing;
pub mod processor_auth;
pub mod upload_auth;
pub mod upload_flow;
pub mod view_auth;

include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

pub fn app() -> axum::Router {
    let public_dir = std::env::var("NEXTRS_PUBLIC_DIR")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/public").to_string());

    let photo_store = photos::PhotoStore::from_env()
        .unwrap_or_else(|error| panic!("invalid photo storage configuration: {error}"));
    let photo_catalog = catalog::PhotoCatalog::from_env()
        .unwrap_or_else(|error| panic!("invalid photo catalog configuration: {error}"));
    let processing_queue = processing::ProcessingQueue::new(photo_catalog.clone());
    let auth_store = auth::AuthStore::from_env()
        .unwrap_or_else(|error| panic!("invalid authentication storage configuration: {error}"));
    let passkey_service = passkeys::PasskeyService::from_env()
        .unwrap_or_else(|error| panic!("invalid passkey configuration: {error}"));
    upload_auth::validate_configuration(&photo_store)
        .unwrap_or_else(|error| panic!("invalid upload authentication configuration: {error}"));
    cron_auth::validate_configuration()
        .unwrap_or_else(|error| panic!("invalid cron authentication configuration: {error}"));
    processor_auth::validate_configuration()
        .unwrap_or_else(|error| panic!("invalid processor authentication configuration: {error}"));
    nextrs::router::build_router_with_public(generated_registry(), &public_dir)
        .merge(nextrs::openapi::spec_router(generated_openapi()))
        .layer(DefaultBodyLimit::max(32 * 1024 * 1024))
        .layer(Extension(photo_store))
        .layer(Extension(photo_catalog))
        .layer(Extension(processing_queue))
        .layer(Extension(passkey_service))
        .layer(Extension(auth_store.clone()))
        .layer(middleware::from_fn_with_state(
            auth_store,
            view_auth::protect,
        ))
}
