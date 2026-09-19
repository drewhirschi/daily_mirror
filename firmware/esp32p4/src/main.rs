//! The ESP32-P4 firmware entry point.
//!
//! There is deliberately almost nothing here. The loop below is the same
//! `daily_mirror_core::runtime` loop the host binary runs — boot once, then
//! step every 20 ms — so every transition already exercised by
//! `firmware/host`'s tests behaves identically on the board. What is specific
//! to the P4 is the six adapters and the order they come up in.
//!
//! Bring-up order (also in README.md): ring and long-press, hosted Wi-Fi join,
//! SoftAP provisioning, claim, fixture upload, camera.

mod button;
mod camera;
mod clock;
mod log as device_log;
mod net;
mod ring;
mod store;

use daily_mirror_core::ports::Store;
use daily_mirror_core::runtime::{Identity, Runtime, Step};
use daily_mirror_core::timing::BUTTON_POLL_MS;
use esp_idf_hal::peripherals::Peripherals;

use button::GpioButton;
use camera::Imx519Camera;
use clock::EspClock;
use device_log::EspLog;
use net::HostedNet;
use ring::Ws2812Ring;
use store::NvsSdStore;

const FIRMWARE_VERSION: &str = env!("CARGO_PKG_VERSION");
const HARDWARE: &str = "esp32-p4-imx519";

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take()?;

    // The button and the ring first: they are the only interface that works
    // with no network, and the ring has to be showing something while the
    // hosted Wi-Fi stack takes its time coming up.
    let button = GpioButton::new(peripherals.pins.gpio4)?;
    let mut ring = Ws2812Ring::new()?;
    ring.spawn_animation_task()?;

    // Then storage, because `boot` needs to know whether credentials exist
    // before it can choose between first-boot pairing and Ready.
    let mut store = NvsSdStore::open()?;
    let identity = Identity {
        device_id: store.device_id()?,
        firmware_version: FIRMWARE_VERSION.to_string(),
        hardware: HARDWARE.to_string(),
    };
    ::log::info!("daily mirror {FIRMWARE_VERSION} on {HARDWARE}, device {}", identity.device_id);

    let net = HostedNet::new()?;
    let camera = Imx519Camera::new()?;

    let mut runtime = Runtime::new(
        identity,
        EspClock,
        button,
        ring,
        camera,
        store,
        net,
        EspLog,
    );

    if runtime.boot() == Step::Reboot {
        reboot();
    }
    loop {
        if runtime.step() == Step::Reboot {
            reboot();
        }
        esp_idf_hal::delay::FreeRtos::delay_ms(BUTTON_POLL_MS as u32);
    }
}

/// A full reset ends here: the store is already erased, so the device comes
/// back up in first-boot pairing.
fn reboot() -> ! {
    ::log::warn!("full reset complete; restarting");
    unsafe { esp_idf_sys::esp_restart() }
}
