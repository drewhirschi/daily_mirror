//! `embuild` drives the ESP-IDF build: it fetches the IDF at the version
//! pinned in Cargo.toml, runs the component manager over the
//! `extra_components` entries, applies `sdkconfig.defaults`, and emits the
//! link arguments `ldproxy` needs.
//!
//! This will not run on a machine without the espup toolchain, which is why
//! this crate is excluded from the firmware workspace and from `just check`.

fn main() {
    embuild::espidf::sysenv::output();
}
