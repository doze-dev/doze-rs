//! The time, as the table asks it: a test makes its own.

use std::fmt;

/// What the table asks the time of: a test makes its own.
pub trait Clock: Send + Sync + fmt::Debug {
    /// Milliseconds on a clock that only goes forward.
    fn mono_ms(&self) -> u64;
    /// Milliseconds since the epoch, for showing.
    fn wall_ms(&self) -> i64;
}

/// The system's clocks.
#[derive(Debug)]
pub struct SystemClock(std::time::Instant);

impl Default for SystemClock {
    fn default() -> Self {
        Self(std::time::Instant::now())
    }
}

impl Clock for SystemClock {
    fn mono_ms(&self) -> u64 {
        u64::try_from(self.0.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn wall_ms(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    }
}
