//! Running work that blocks (a flush, a directory made) without stalling the runtime thread it is on.

use tokio::runtime::{Handle, RuntimeFlavor};

/// Runs `f`. On a multi-threaded runtime the worker hands its other tasks to another thread first, so a flush does
/// not hold up every connection that worker would have served. A current-thread runtime (tests) has no other
/// thread to hand them to, and `f` just runs.
pub(crate) fn run<T>(f: impl FnOnce() -> T) -> T {
    match Handle::try_current().map(|h| h.runtime_flavor()) {
        Ok(RuntimeFlavor::MultiThread) => tokio::task::block_in_place(f),
        _ => f(),
    }
}
