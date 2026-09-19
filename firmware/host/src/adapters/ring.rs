//! Ring: the terminal. One line per change, timestamped, so a live run reads
//! like the LED behaviour the plan describes.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use daily_mirror_core::ports::Ring;
use daily_mirror_core::ring::RingPattern;

#[derive(Debug)]
pub struct TerminalRing {
    start: Instant,
    current: Option<RingPattern>,
    /// Every pattern the ring was actually set to, for tests.
    history: Arc<Mutex<Vec<RingPattern>>>,
    quiet: bool,
}

impl TerminalRing {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            current: None,
            history: Arc::new(Mutex::new(Vec::new())),
            quiet: false,
        }
    }

    /// A ring that records but does not print. Used by the tests.
    pub fn quiet() -> Self {
        Self {
            quiet: true,
            ..Self::new()
        }
    }

    /// A handle to the pattern history, shared with the ring.
    pub fn history(&self) -> Arc<Mutex<Vec<RingPattern>>> {
        Arc::clone(&self.history)
    }
}

impl Default for TerminalRing {
    fn default() -> Self {
        Self::new()
    }
}

impl Ring for TerminalRing {
    fn set(&mut self, pattern: RingPattern) {
        if self.current == Some(pattern) {
            return;
        }
        self.current = Some(pattern);
        if let Ok(mut history) = self.history.lock() {
            history.push(pattern);
        }
        if !self.quiet {
            let seconds = self.start.elapsed().as_secs_f64();
            println!("[{seconds:8.3}s] ring -> {pattern:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_patterns_are_one_change() {
        let mut ring = TerminalRing::quiet();
        let history = ring.history();
        ring.set(RingPattern::AmberChase);
        ring.set(RingPattern::AmberChase);
        ring.set(RingPattern::SolidWhite);
        assert_eq!(
            *history.lock().unwrap(),
            vec![RingPattern::AmberChase, RingPattern::SolidWhite]
        );
    }
}
