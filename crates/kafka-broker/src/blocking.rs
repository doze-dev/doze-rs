//! Running work that blocks (a flush, a directory made) without stalling the runtime thread it is on.
//!
//! Two ways, for two kinds of caller. A request (a produce's flush, a partition opened for a fetch) uses [`run`]: the work is
//! done where it is, now, and the worker's other tasks move to another thread meanwhile. A background chore (the cleaner, the
//! retention and offsets sweeps) uses [`in_background`]: the work goes to the blocking pool and the chore waits for it.
//!
//! Why two. `block_in_place` hands the worker's core to a thread it starts for the purpose, and the thread it leaves becomes
//! an idle blocking thread that lingers for the pool's keep-alive (10 s): measured on an idle broker, each sweep took the
//! process from 3 threads to 5 and back ten seconds later. A request pays that once under load, when the threads are reused;
//! a chore paid it on every pass. `spawn_blocking` starts at most one pool thread, reuses it for every step of the pass, and
//! the runtime's own workers are never swapped.

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

/// Runs `f` on the blocking pool and waits for it, for a background chore. Outside a multi-threaded runtime (tests) it just
/// runs, as [`run`] does: the tests' current-thread runtime would otherwise start a thread for every step.
pub(crate) async fn in_background<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    match Handle::try_current().map(|h| h.runtime_flavor()) {
        Ok(RuntimeFlavor::MultiThread) => match tokio::task::spawn_blocking(f).await {
            Ok(out) => out,
            // The work panicked (or the runtime is going away): the chore goes the way the work did.
            Err(e) => std::panic::resume_unwind(
                e.try_into_panic()
                    .unwrap_or_else(|e| Box::new(e.to_string())),
            ),
        },
        _ => f(),
    }
}
