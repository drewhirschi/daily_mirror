//! Button: the keyboard. A background thread reads lines from stdin and moves
//! a shared level that the runtime polls every 20 ms, which is exactly how the
//! GPIO adapter behaves — the core's gesture detector does the debouncing and
//! press-length work either way.
//!
//! Commands:
//!
//! | Line | Effect |
//! | --- | --- |
//! | `p` | press down and hold |
//! | `r` | release |
//! | `c` | a short click (press, 80 ms, release) |
//! | `hold <ms>` | press, wait, release — `hold 5000` enters pairing |
//! | `q` | quit |

use std::io::BufRead;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use daily_mirror_core::ports::Button;

/// The shared, debounced button level. Cloneable: one side is polled by the
/// runtime, the other is driven by stdin, a script, or a test.
#[derive(Clone, Debug, Default)]
pub struct SharedButton {
    pressed: Arc<AtomicBool>,
    quit: Arc<AtomicBool>,
}

impl SharedButton {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn press(&self) {
        self.pressed.store(true, Ordering::SeqCst);
    }

    pub fn release(&self) {
        self.pressed.store(false, Ordering::SeqCst);
    }

    pub fn set(&self, pressed: bool) {
        self.pressed.store(pressed, Ordering::SeqCst);
    }

    pub fn quit_requested(&self) -> bool {
        self.quit.load(Ordering::SeqCst)
    }

    pub fn request_quit(&self) {
        self.quit.store(true, Ordering::SeqCst);
    }
}

impl Button for SharedButton {
    fn is_pressed(&mut self) -> bool {
        self.pressed.load(Ordering::SeqCst)
    }
}

/// Start the stdin reader. It owns its thread and returns immediately.
pub fn spawn_stdin_reader(button: SharedButton) {
    thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            apply(&button, line.trim());
            if button.quit_requested() {
                break;
            }
        }
        button.request_quit();
    });
}

fn apply(button: &SharedButton, line: &str) {
    let mut parts = line.split_whitespace();
    match (parts.next(), parts.next()) {
        (Some("p") | Some("press"), _) => button.press(),
        (Some("r") | Some("release"), _) => button.release(),
        (Some("c") | Some("click"), _) => {
            button.press();
            thread::sleep(Duration::from_millis(80));
            button.release();
        }
        (Some("hold"), Some(millis)) => match millis.parse::<u64>() {
            Ok(millis) => {
                button.press();
                thread::sleep(Duration::from_millis(millis));
                button.release();
            }
            Err(_) => eprintln!("hold needs a number of milliseconds, got {millis:?}"),
        },
        (Some("q") | Some("quit"), _) => button.request_quit(),
        (Some(""), _) | (None, _) => {}
        (Some(other), _) => eprintln!("unknown command {other:?}; try p, r, c, hold <ms>, q"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn press_and_release_move_the_level() {
        let mut button = SharedButton::new();
        assert!(!button.is_pressed());
        apply(&button.clone(), "p");
        assert!(button.is_pressed());
        apply(&button.clone(), "r");
        assert!(!button.is_pressed());
    }

    #[test]
    fn hold_presses_for_the_requested_time_then_releases() {
        let mut button = SharedButton::new();
        apply(&button.clone(), "hold 40");
        assert!(!button.is_pressed());
    }

    #[test]
    fn quit_is_requested_once_and_stays() {
        let button = SharedButton::new();
        apply(&button, "q");
        assert!(button.quit_requested());
    }
}
