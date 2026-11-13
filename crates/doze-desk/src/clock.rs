//! The hub's clock: wall time for what a page shows ("up 14m", "retrying in 3s"), and a monotonic instant derived
//! from it for working out rates, so a test that fixes the time fixes both.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

type NowFn = dyn Fn() -> SystemTime + Send + Sync;

/// What time it is.
#[derive(Clone)]
pub struct Clock {
    now: Arc<NowFn>,
    /// When the clock was made, on both clocks: an instant is the base plus however far the wall time has moved on.
    base: (Instant, SystemTime),
}

impl fmt::Debug for Clock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Clock").field("now", &self.now()).finish()
    }
}

impl Default for Clock {
    fn default() -> Self {
        Clock::system()
    }
}

impl Clock {
    /// The system's clock.
    pub fn system() -> Clock {
        Clock::from_fn(SystemTime::now)
    }

    /// A clock that always says `t`.
    pub fn fixed(t: SystemTime) -> Clock {
        Clock::from_fn(move || t)
    }

    /// A clock that asks `now`.
    pub fn from_fn(now: impl Fn() -> SystemTime + Send + Sync + 'static) -> Clock {
        let base = (Instant::now(), now());
        Clock {
            now: Arc::new(now),
            base,
        }
    }

    /// The wall time.
    pub fn now(&self) -> SystemTime {
        (self.now)()
    }

    /// A monotonic instant that moves as the wall time does (and not at all for a fixed clock).
    pub fn instant(&self) -> Instant {
        let moved = self
            .now()
            .duration_since(self.base.1)
            .unwrap_or(Duration::ZERO);
        self.base.0.checked_add(moved).unwrap_or(self.base.0)
    }
}
