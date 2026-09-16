//! Log: stderr, plus an optional in-memory transcript the tests assert on.

use std::sync::{Arc, Mutex};

use daily_mirror_core::runtime::Log;

#[derive(Clone, Debug)]
pub struct HostLog {
    lines: Arc<Mutex<Vec<String>>>,
    print: bool,
}

impl HostLog {
    pub fn new() -> Self {
        Self {
            lines: Arc::new(Mutex::new(Vec::new())),
            print: true,
        }
    }

    /// Records without printing. Used by the tests.
    pub fn quiet() -> Self {
        Self {
            print: false,
            ..Self::new()
        }
    }

    /// A handle to the transcript, shared with the log.
    pub fn lines(&self) -> Arc<Mutex<Vec<String>>> {
        Arc::clone(&self.lines)
    }
}

impl Default for HostLog {
    fn default() -> Self {
        Self::new()
    }
}

impl Log for HostLog {
    fn log(&mut self, message: &str) {
        if self.print {
            eprintln!("device: {message}");
        }
        if let Ok(mut lines) = self.lines.lock() {
            lines.push(message.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_recorded_in_order() {
        let mut log = HostLog::quiet();
        let lines = log.lines();
        log.log("one");
        log.log("two");
        assert_eq!(*lines.lock().unwrap(), vec!["one", "two"]);
    }
}
