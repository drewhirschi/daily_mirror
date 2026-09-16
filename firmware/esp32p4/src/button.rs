//! Button: one GPIO, active low with the internal pull-up, exactly like the
//! Pi rig's wiring.
//!
//! The core's `GestureDetector` does press-length and click-sequence work, so
//! this adapter only has to answer "is it down right now" without chattering.
//! It debounces the way the Pi does: a level change is only believed once it
//! has held for `DEBOUNCE_MS`.
//!
//! Bring-up step one. This and the ring must work before the Wi-Fi stack
//! exists, which is why `main` initializes them first.

use daily_mirror_core::Millis;
use daily_mirror_core::ports::{Button, Clock};
use daily_mirror_core::timing::DEBOUNCE_MS;
use esp_idf_hal::gpio::{Input, PinDriver};

use crate::clock::EspClock;

/// GPIO the button is wired to. Active low: pressed pulls it to ground.
pub const BUTTON_PIN: u32 = 4;

pub struct GpioButton<'d, P>
where
    P: esp_idf_hal::gpio::Pin,
{
    pin: PinDriver<'d, P, Input>,
    clock: EspClock,
    stable: bool,
    candidate: bool,
    changed_at: Millis,
}

impl<'d, P> GpioButton<'d, P>
where
    P: esp_idf_hal::gpio::InputPin,
{
    /// Configure the pin as an input with the internal pull-up enabled.
    ///
    /// ```ignore
    /// let peripherals = Peripherals::take()?;
    /// let button = GpioButton::new(peripherals.pins.gpio4)?;
    /// ```
    pub fn new(_pin: impl esp_idf_hal::peripheral::Peripheral<P = P> + 'd) -> anyhow::Result<Self> {
        // FFI: PinDriver::input(pin)? then pin.set_pull(Pull::Up)?.
        todo!("configure {BUTTON_PIN} as a pulled-up input and seed the debounce state")
    }

    /// The raw, undebounced level. Active low, so a low pin means pressed.
    fn raw(&self) -> bool {
        self.pin.is_low()
    }
}

impl<P> Button for GpioButton<'_, P>
where
    P: esp_idf_hal::gpio::InputPin,
{
    fn is_pressed(&mut self) -> bool {
        let now = self.clock.now();
        let raw = self.raw();
        if raw != self.candidate {
            self.candidate = raw;
            self.changed_at = now;
        } else if raw != self.stable && now.saturating_sub(self.changed_at) >= DEBOUNCE_MS {
            self.stable = raw;
        }
        self.stable
    }
}
