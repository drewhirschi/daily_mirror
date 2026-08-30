fn main() {
    nextrs::build::emit_registry("app", "src/app.rs", "nextrs_routes.rs")
        .expect("nextrs::build::emit_registry failed");

    nextrs::bundle::bundle_pages(&nextrs::bundle::BundleConfig {
        app_dir: "app",
        project_dir: Some("."),
        client_dir: ".nextrs/client",
        client_alias: "@server/client",
        public_dist: "public/dist",
        ..Default::default()
    })
    .expect("nextrs::bundle::bundle_pages failed");
}
