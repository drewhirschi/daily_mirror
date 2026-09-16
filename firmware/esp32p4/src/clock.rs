//! Clock: `esp_timer_get_time`, the monotonic microsecond counter that keeps
//! running across light sleep. The one adapter with no `todo!()` — the FFI is
//! a single call and there is nothing to get wrong.

use daily_mirror_core::Millis;
use daily_mirror_core::ports::Clock;

#[derive(Debug, Default)]
pub struct EspClock;

impl Clock for EspClock {
    fn now(&self) -> Millis {
        // Safe: reads a counter, takes no arguments, cannot fail.
        let micros = unsafe { esp_idf_sys::esp_timer_get_time() };
        Millis((micros.max(0) as u64) / 1_000)
    }
}
