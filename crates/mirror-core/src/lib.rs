//! Platform-neutral core for the Daily Mirror capture appliance.
//!
//! Everything here is pure logic with no I/O: the device state machine, the
//! button gesture detector, the ring vocabulary, and the request/response
//! models shared with the server. Adapters (host, ESP32-P4) implement the
//! traits in [`ports`] and drive [`state::Machine`] with events, executing the
//! [`state::Command`]s it returns.
//!
//! See `docs/device-pairing-plan.md` for the design this crate encodes.

pub mod button;
pub mod contract;
pub mod ports;
pub mod ring;
pub mod state;

/// Milliseconds since an arbitrary monotonic origin. Adapters supply this from
/// their own clock; the core never reads a clock itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Millis(pub u64);

impl Millis {
    pub const fn saturating_sub(self, other: Millis) -> u64 {
        self.0.saturating_sub(other.0)
    }
}

/// Timing constants that define the physical interaction. Copied from the Pi
/// device crate so both platforms behave identically.
pub mod timing {
    pub const BUTTON_POLL_MS: u64 = 20;
    pub const DEBOUNCE_MS: u64 = 60;
    /// Ring starts filling amber, telling the user a long-press is in progress.
    pub const HOLD_ARM_MS: u64 = 2_000;
    /// Enter pairing.
    pub const HOLD_PAIR_MS: u64 = 5_000;
    /// Full reset becomes armed; ring goes solid red.
    pub const HOLD_RESET_MS: u64 = 20_000;
    /// Window after a 20 s hold in which three clicks complete a full reset.
    pub const RESET_CLICK_WINDOW_MS: u64 = 3_000;
    pub const RESET_CLICK_COUNT: u8 = 3;
    /// First-boot pairing window and the pairing timeout after a long-press.
    pub const PAIRING_TIMEOUT_MS: u64 = 5 * 60 * 1_000;
    /// How long the device waits for the confirming press after credentials arrive.
    pub const CONFIRM_TIMEOUT_MS: u64 = 30_000;
    /// Countdown before the shutter, matching the Pi's three one-second pulses.
    pub const COUNTDOWN_MS: u64 = 3_000;
}
