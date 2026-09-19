//! Log: the ESP-IDF log, so the runtime's narration shows up in
//! `cargo espflash monitor` next to the C-side messages.

use daily_mirror_core::runtime::Log;

#[derive(Debug, Default)]
pub struct EspLog;

impl Log for EspLog {
    fn log(&mut self, message: &str) {
        log::info!("{message}");
    }
}
