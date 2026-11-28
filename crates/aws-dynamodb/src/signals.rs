//! Wake-ups by stream. A consumer that waits for a stream's records (a Lambda event source mapping) watches that stream, and is
//! woken when a call appends to it; an append to another stream does not wake it. An entry exists only while someone watches.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::Notify;
use tokio::sync::futures::Notified;

/// The watched streams, by key.
#[derive(Debug, Default)]
pub(crate) struct Signals {
    watched: Mutex<HashMap<String, Arc<Notify>>>,
}

impl Signals {
    /// Starts watching `key`.
    pub(crate) fn watch(self: &Arc<Self>, key: String) -> Signal {
        let notify = {
            let mut watched = self.watched.lock().unwrap_or_else(PoisonError::into_inner);
            Arc::clone(watched.entry(key.clone()).or_default())
        };
        Signal {
            signals: Arc::clone(self),
            key,
            notify,
        }
    }

    /// Wakes whoever watches `key`; nothing when nobody does.
    pub(crate) fn tell(&self, key: &str) {
        let notify = self
            .watched
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .cloned();
        if let Some(notify) = notify {
            notify.notify_waiters();
        }
    }

    /// How many keys are watched (for tests).
    #[cfg(test)]
    pub(crate) fn watched(&self) -> usize {
        self.watched
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

/// A watch on one stream, ended when dropped.
#[derive(Debug)]
pub struct Signal {
    signals: Arc<Signals>,
    key: String,
    notify: Arc<Notify>,
}

impl Signal {
    /// A wake-up for the next append. Call `enable` on it (pinned) before reading, so an append between the read and the wait is
    /// not missed.
    pub fn notified(&self) -> Notified<'_> {
        self.notify.notified()
    }
}

impl Drop for Signal {
    fn drop(&mut self) {
        let mut watched = self
            .signals
            .watched
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // The map's and this one: no one else watches.
        if Arc::strong_count(&self.notify) == 2 {
            watched.remove(&self.key);
        }
    }
}
