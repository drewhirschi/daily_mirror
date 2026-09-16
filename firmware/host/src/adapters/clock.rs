//! Clock: the monotonic `std::time::Instant` for a live run, and a virtual
//! clock for `--script` mode so a five-minute pairing timeout takes no real
//! time in `cargo test`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use daily_mirror_core::Millis;
use daily_mirror_core::ports::Clock;

/// A virtual clock's time source. Cloneable so the script driver can advance
/// the same time the runtime reads.
#[derive(Clone, Debug, Default)]
pub struct VirtualTime(Arc<AtomicU64>);

impl VirtualTime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self, millis: u64) {
        self.0.store(millis, Ordering::SeqCst);
    }

    pub fn advance(&self, millis: u64) {
        self.0.fetch_add(millis, Ordering::SeqCst);
    }

    pub fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Debug)]
pub enum HostClock {
    System(Instant),
    Virtual(VirtualTime),
}

impl HostClock {
    pub fn system() -> Self {
        Self::System(Instant::now())
    }

    pub fn virtual_clock(time: VirtualTime) -> Self {
        Self::Virtual(time)
    }
}

impl Clock for HostClock {
    fn now(&self) -> Millis {
        match self {
            Self::System(start) => Millis(start.elapsed().as_millis() as u64),
            Self::Virtual(time) => Millis(time.now_ms()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_time_advances_only_when_told() {
        let time = VirtualTime::new();
        let clock = HostClock::virtual_clock(time.clone());
        assert_eq!(clock.now(), Millis(0));
        time.advance(20);
        assert_eq!(clock.now(), Millis(20));
        time.set(5_000);
        assert_eq!(clock.now(), Millis(5_000));
    }
}
