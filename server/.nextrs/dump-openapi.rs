// @generated nextrs helper. Application code belongs in src/app.rs.
fn main() {
    let spec = server::generated_openapi();
    let json = spec.to_pretty_json().expect("serialize OpenAPI document");
    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/.nextrs/openapi.json");
    std::fs::write(out, json).expect("write .nextrs/openapi.json");
    eprintln!("wrote {out}");
}
