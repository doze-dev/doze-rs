//! Who waits on which queue. A receive that waits (a long poll, or a Lambda event source mapping between reads) watches its
//! queue, and is woken when a call there adds a message or makes one show sooner; a call on another queue does not wake it. The
//! watch also keeps when the queue's first hidden message shows, as the last receive that found nothing was told, so a waiter
//! sleeps until then and not a moment less. An entry exists only while someone watches: a queue nobody waits on costs nothing.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use doze_runtime::Clock;
use tokio::sync::Notify;
use tokio::sync::futures::Notified;

/// The watched queues, by `region/name`.
pub(crate) struct Arrivals {
    clock: Arc<dyn Clock>,
    watched: Mutex<HashMap<String, Arc<Entry>>>,
}

impl std::fmt::Debug for Arrivals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Arrivals").finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct Entry {
    notify: Notify,
    /// When the queue's first hidden message shows, in the clock's milliseconds; `i64::MAX` when none is hidden or it is not
    /// known yet.
    next_visible_ms: AtomicI64,
}

/// The key of a queue: queues are kept per region, by name.
pub(crate) fn key(region: &str, name: &str) -> String {
    format!("{region}/{name}")
}

impl Arrivals {
    pub(crate) fn new(clock: Arc<dyn Clock>) -> Arc<Self> {
        Arc::new(Self {
            clock,
            watched: Mutex::new(HashMap::new()),
        })
    }

    /// Starts watching the queue `key` names.
    pub(crate) fn watch(self: &Arc<Self>, key: String) -> Watch {
        let entry = {
            let mut watched = self.watched.lock().unwrap_or_else(PoisonError::into_inner);
            Arc::clone(watched.entry(key.clone()).or_insert_with(|| {
                Arc::new(Entry {
                    notify: Notify::new(),
                    next_visible_ms: AtomicI64::new(i64::MAX),
                })
            }))
        };
        Watch {
            arrivals: Arc::clone(self),
            key,
            entry,
        }
    }

    /// Wakes whoever waits on the queue `key` names; nothing when nobody does.
    pub(crate) fn tell(&self, key: &str) {
        let entry = self
            .watched
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .cloned();
        if let Some(entry) = entry {
            entry.next_visible_ms.store(i64::MAX, Ordering::Relaxed);
            entry.notify.notify_waiters();
        }
    }

    /// Notes what a receive that found nothing was told about when the queue's first hidden message shows.
    pub(crate) fn note(&self, key: &str, next_visible_ms: Option<i64>) {
        let watched = self.watched.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(entry) = watched.get(key) {
            entry
                .next_visible_ms
                .store(next_visible_ms.unwrap_or(i64::MAX), Ordering::Relaxed);
        }
    }

    /// How many queues are watched (for tests).
    #[cfg(test)]
    pub(crate) fn watched(&self) -> usize {
        self.watched
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

/// A watch on one queue, ended when dropped.
#[derive(Debug)]
pub struct Watch {
    arrivals: Arc<Arrivals>,
    key: String,
    entry: Arc<Entry>,
}

impl Watch {
    /// A wake-up for the next arrival. Call `enable` on it (pinned) before reading the queue, so an arrival between the read and
    /// the wait is not missed.
    pub fn notified(&self) -> Notified<'_> {
        self.entry.notify.notified()
    }

    /// How long until the queue's first hidden message shows (in flight or delayed), as the last receive that found nothing was
    /// told; `None` when none is hidden, so only an arrival can give a waiter something.
    #[must_use]
    pub fn until_visible(&self) -> Option<Duration> {
        let at = self.entry.next_visible_ms.load(Ordering::Relaxed);
        if at == i64::MAX {
            return None;
        }
        let ms = at.saturating_sub(self.arrivals.clock.now_ms()).max(1);
        Some(Duration::from_millis(u64::try_from(ms).unwrap_or(1)))
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let mut watched = self
            .arrivals
            .watched
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // The map's and this one: no one else watches.
        if Arc::strong_count(&self.entry) == 2 {
            watched.remove(&self.key);
        }
    }
}
