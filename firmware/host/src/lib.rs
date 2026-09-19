//! Linux adapters for the Daily Mirror device runtime.
//!
//! Every trait in `daily_mirror_core::ports` gets a real implementation here:
//! the button is the keyboard, the ring is the terminal, the camera is a
//! fixture JPEG, the store is a directory, and the network is `reqwest` plus a
//! small local HTTP server standing in for the board's SoftAP provisioning
//! link. The driver loop itself lives in `daily_mirror_core::runtime` and is
//! shared with the ESP32-P4 firmware, so what runs under `cargo test` here is
//! the same loop that will run on the board.

pub mod adapters;
pub mod script;

use std::path::PathBuf;

use daily_mirror_core::runtime::{Identity, Runtime};

use adapters::button::SharedButton;
use adapters::camera::FixtureCamera;
use adapters::clock::HostClock;
use adapters::log::HostLog;
use adapters::net::HttpNet;
use adapters::ring::TerminalRing;
use adapters::store::DirStore;

/// The firmware version reported in the claim.
pub const FIRMWARE_VERSION: &str = env!("CARGO_PKG_VERSION");
/// The hardware label reported in the claim.
pub const HARDWARE: &str = "host";

/// The host runtime with every adapter bound.
pub type HostRuntime =
    Runtime<HostClock, SharedButton, TerminalRing, FixtureCamera, DirStore, HttpNet, HostLog>;

/// Everything needed to build a [`HostRuntime`].
pub struct HostConfig {
    pub store_dir: PathBuf,
    pub fixture: PathBuf,
    pub provisioning_port: u16,
    pub clock: HostClock,
    pub button: SharedButton,
    pub log: HostLog,
}

/// Build the runtime. The store is created if missing and mints the device id
/// on first use, exactly as the board's NVS adapter will.
pub fn build(config: HostConfig) -> anyhow::Result<HostRuntime> {
    let mut store = DirStore::open(&config.store_dir)?;
    let identity = Identity {
        device_id: store.device_id_owned()?,
        firmware_version: FIRMWARE_VERSION.to_string(),
        hardware: HARDWARE.to_string(),
    };
    Ok(Runtime::new(
        identity,
        config.clock,
        config.button,
        TerminalRing::new(),
        FixtureCamera::new(config.fixture),
        store,
        HttpNet::new(config.provisioning_port),
        config.log,
    ))
}
