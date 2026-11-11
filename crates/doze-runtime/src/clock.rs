//! Time as a value that is passed in.
//!
//! A service that reads the wall clock directly cannot be tested for what happens at a deadline.
//! Services take a [`Clock`]; tests give them a [`ManualClock`] and move it by hand. (Timers are not
//! the clock's business: those are [`Expiring`](crate::Expiring), which tokio's paused time drives.)

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A source of the current time, in milliseconds since the Unix epoch.
pub trait Clock: Send + Sync {
    /// Now, in milliseconds since the epoch.
    fn now_ms(&self) -> i64;
}

/// The wall clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        // A clock set before 1970 reads as the epoch; a duration past `i64::MAX` ms is not a real date.
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    }
}

/// A clock that moves only when told to. Shared by reference between a test and the service it drives.
#[derive(Debug, Default)]
pub struct ManualClock(AtomicI64);

impl ManualClock {
    /// A clock reading `now_ms`.
    #[must_use]
    pub fn at(now_ms: i64) -> Self {
        Self(AtomicI64::new(now_ms))
    }

    /// Sets the time.
    pub fn set(&self, now_ms: i64) {
        self.0.store(now_ms, Ordering::SeqCst);
    }

    /// Moves the time forward (or back, for a negative step).
    pub fn advance(&self, by_ms: i64) {
        self.0.fetch_add(by_ms, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manual_clock_moves_only_when_told() {
        let clock = ManualClock::at(1_000);
        assert_eq!(clock.now_ms(), 1_000);
        clock.advance(250);
        assert_eq!(clock.now_ms(), 1_250);
        clock.set(5);
        assert_eq!(clock.now_ms(), 5);
    }

    #[test]
    fn the_system_clock_is_after_2023() {
        assert!(SystemClock.now_ms() > 1_700_000_000_000);
    }
}
