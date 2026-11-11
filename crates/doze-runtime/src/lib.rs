//! What every doze service runs on.
//!
//! - [`Lazy`]: a service that costs nothing until it is used and nothing again once it has been idle.
//! - [`Expiring`]: the one timer mechanism. A set of deadlines that holds no timer while it is empty.
//! - [`Clock`]: time as a value passed in, so tests own the clock.
//! - [`Tasks`]: tracked tasks and a clean stop.
//! - [`DataDirLock`]: one process per data directory.
//!
//! The lifecycle and timer patterns come from spike C (2026-11-08); the lock and the stop handle are
//! extracted from `doze-kafka` and `kafka-server` (A1).

mod clock;
mod expiring;
mod lazy;
mod lock;
mod tasks;

pub use clock::{Clock, ManualClock, SystemClock};
pub use expiring::Expiring;
pub use lazy::{Ctx, Error, Lazy, Service};
pub use lock::{DataDirLock, LockError};
pub use tasks::Tasks;
