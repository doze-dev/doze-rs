//! The pool's limits: at most [`MAX_WORKERS`] processes per function version, and at most `RuntimeConfig::max_processes` in all.
//! When the pool is full, a new process takes the place of the one that has waited longest for an invocation; when every process is
//! working, it waits for one to come free, for at most [`PLACE_WAIT`], and the invocation is then refused as throttled (as Lambda
//! refuses one past the account's concurrency). A version's entry goes once nothing of it is alive or waiting, so a function updated a
//! thousand times does not keep a thousand entries.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};

use super::worker::Worker;

/// How many processes serve one function version at once; further invocations wait for one.
pub(crate) const MAX_WORKERS: usize = 8;
/// How long a start waits for a place when every process is working.
pub(crate) const PLACE_WAIT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub(crate) struct Slot {
    pub(crate) workers: Mutex<Vec<Arc<Worker>>>,
    pub(crate) permits: Arc<Semaphore>,
}

impl Slot {
    /// Nothing of it is alive, and no invocation holds or waits for a permit.
    fn unused(&self) -> bool {
        self.permits.available_permits() == MAX_WORKERS
            && self
                .workers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .all(|w| w.is_dead())
    }
}

/// The processes of every function version, and the places they take.
#[derive(Debug)]
pub(crate) struct Pool {
    slots: Mutex<HashMap<String, Arc<Slot>>>,
    places: Arc<Semaphore>,
    /// Told when a process comes free (it may be ended to make room).
    freed: Notify,
}

impl Pool {
    pub(crate) fn new(max_processes: usize) -> Self {
        Self {
            slots: Mutex::new(HashMap::new()),
            places: Arc::new(Semaphore::new(max_processes.max(1))),
            freed: Notify::new(),
        }
    }

    /// The version's entry; a new one first clears the entries nothing uses.
    pub(crate) fn slot(&self, key: &str) -> Arc<Slot> {
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(s) = slots.get(key) {
            return Arc::clone(s);
        }
        slots.retain(|_, s| !s.unused());
        let s = Arc::new(Slot {
            workers: Mutex::new(Vec::new()),
            permits: Arc::new(Semaphore::new(MAX_WORKERS)),
        });
        slots.insert(key.to_owned(), Arc::clone(&s));
        s
    }

    /// How many version entries are kept (for tests).
    #[cfg(test)]
    pub(crate) fn slots(&self) -> usize {
        self.slots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Calls `f` with every process of the versions whose key starts with `prefix`.
    pub(crate) fn each(&self, prefix: &str, mut f: impl FnMut(&Arc<Worker>)) {
        let slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        for (_, slot) in slots.iter().filter(|(k, _)| k.starts_with(prefix)) {
            slot.workers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .for_each(&mut f);
        }
    }

    /// A process has come free.
    pub(crate) fn notify_free(&self) {
        self.freed.notify_waiters();
    }

    /// A place for a new process: a free one, or the place of the process idle longest (it is ended), or, when every process is
    /// working, the first that comes free within [`PLACE_WAIT`]. `None` when none did.
    pub(crate) async fn place(&self) -> Option<OwnedSemaphorePermit> {
        let deadline = tokio::time::Instant::now() + PLACE_WAIT;
        loop {
            let freed = self.freed.notified();
            tokio::pin!(freed);
            freed.as_mut().enable();
            if let Ok(p) = Arc::clone(&self.places).try_acquire_owned() {
                return Some(p);
            }
            if self.end_longest_idle() {
                // Its place comes back once the process has gone.
                return tokio::time::timeout_at(deadline, Arc::clone(&self.places).acquire_owned())
                    .await
                    .ok()
                    .and_then(Result::ok);
            }
            let more = tokio::select! {
                () = &mut freed => true,
                p = Arc::clone(&self.places).acquire_owned() => return p.ok(),
                () = tokio::time::sleep_until(deadline) => false,
            };
            if !more {
                return None;
            }
        }
    }

    /// Ends the process that has waited longest for an invocation; `false` when every process is working.
    fn end_longest_idle(&self) -> bool {
        let mut idle: Vec<(i64, Arc<Worker>)> = Vec::new();
        self.each("", |w| {
            if let Some(since) = w.idle_since() {
                idle.push((since, Arc::clone(w)));
            }
        });
        idle.sort_by_key(|(since, _)| *since);
        // Claimed first, so no invocation is handed to it on the way out.
        let Some((_, w)) = idle.into_iter().find(|(_, w)| w.claim()) else {
            return false;
        };
        tracing::debug!(function = %w.spec.name, "a warm function process is ended to make room for another");
        w.kill();
        true
    }
}
