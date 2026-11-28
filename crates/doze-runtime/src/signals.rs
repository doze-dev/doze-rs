//! Wake-ups by key: a consumer that waits for something to happen to one thing (a stream's records, a queue's messages) watches its
//! key, and is woken when a call says that thing changed; a change to another key wakes nobody. Nothing polls, and an entry exists
//! only while someone watches: a key nobody waits on costs nothing.
//!
//! A change may name the parts it touched (a stream's shards, [`Signals::tell_parts`]): each part's count of changes is kept while
//! the key is watched, and a consumer that remembers the counts it last read reads only the parts whose count moved
//! ([`Signal::count`]). A key may also carry a value of the service's own beside its wake-up (`E`, [`Signal::extra`]: SQS keeps when
//! the queue's first hidden message shows).
//!
//! Was three copies: Kinesis's and DynamoDB's stream signals and SQS's arrivals (workstream 1); one now.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::Notify;
use tokio::sync::futures::Notified;

/// The watched keys.
pub struct Signals<E = ()> {
    watched: Mutex<HashMap<String, Arc<Entry<E>>>>,
}

struct Entry<E> {
    notify: Notify,
    /// Changes by part, counted from when the key was first watched.
    parts: Mutex<HashMap<String, u64>>,
    extra: E,
}

impl<E> std::fmt::Debug for Signals<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Signals")
            .field("watched", &self.watched())
            .finish()
    }
}

impl<E> Default for Signals<E> {
    fn default() -> Self {
        Self {
            watched: Mutex::new(HashMap::new()),
        }
    }
}

impl<E: Default + Send + Sync + 'static> Signals<E> {
    /// Starts watching `key`.
    pub fn watch(self: &Arc<Self>, key: String) -> Signal<E> {
        let entry = {
            let mut watched = self.lock();
            Arc::clone(watched.entry(key.clone()).or_insert_with(|| {
                Arc::new(Entry {
                    notify: Notify::new(),
                    parts: Mutex::new(HashMap::new()),
                    extra: E::default(),
                })
            }))
        };
        Signal {
            signals: Arc::clone(self),
            key,
            entry,
        }
    }
}

impl<E> Signals<E> {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<Entry<E>>>> {
        self.watched.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn entry(&self, key: &str) -> Option<Arc<Entry<E>>> {
        self.lock().get(key).cloned()
    }

    /// Wakes whoever watches `key`; nothing when nobody does.
    pub fn tell(&self, key: &str) {
        if let Some(entry) = self.entry(key) {
            entry.notify.notify_waiters();
        }
    }

    /// Counts a change to each of `parts` of `key`, and wakes whoever watches it; nothing when nobody does.
    pub fn tell_parts<'p>(&self, key: &str, parts: impl IntoIterator<Item = &'p str>) {
        let Some(entry) = self.entry(key) else {
            return;
        };
        {
            let mut counts = entry.parts.lock().unwrap_or_else(PoisonError::into_inner);
            for part in parts {
                *counts.entry(part.to_owned()).or_default() += 1;
            }
        }
        entry.notify.notify_waiters();
    }

    /// Runs `f` with the value `key` carries, when someone watches it.
    pub fn with_extra(&self, key: &str, f: impl FnOnce(&E)) {
        if let Some(entry) = self.entry(key) {
            f(&entry.extra);
        }
    }

    /// How many keys are watched.
    #[must_use]
    pub fn watched(&self) -> usize {
        self.lock().len()
    }
}

/// A watch on one key, ended when dropped.
pub struct Signal<E = ()> {
    signals: Arc<Signals<E>>,
    key: String,
    entry: Arc<Entry<E>>,
}

impl<E> std::fmt::Debug for Signal<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Signal").field("key", &self.key).finish()
    }
}

impl<E> Signal<E> {
    /// A wake-up for the next change. Call `enable` on it (pinned) before reading, so a change between the read and the wait is
    /// not missed.
    pub fn notified(&self) -> Notified<'_> {
        self.entry.notify.notified()
    }

    /// How many changes `part` has had since the key was first watched (0: none, or none counted).
    #[must_use]
    pub fn count(&self, part: &str) -> u64 {
        self.entry
            .parts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(part)
            .copied()
            .unwrap_or(0)
    }

    /// Every part's count of changes since the key was first watched: a consumer finds parts it does not know of yet here.
    #[must_use]
    pub fn counts(&self) -> HashMap<String, u64> {
        self.entry
            .parts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The value the key carries.
    #[must_use]
    pub fn extra(&self) -> &E {
        &self.entry.extra
    }
}

impl<E> Drop for Signal<E> {
    fn drop(&mut self) {
        let mut watched = self.signals.lock();
        // The map's and this one: no one else watches.
        if Arc::strong_count(&self.entry) == 2 {
            watched.remove(&self.key);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[tokio::test]
    async fn a_watch_is_woken_by_its_key_counts_its_parts_and_leaves_no_entry() {
        let signals: Arc<Signals> = Arc::default();
        signals.tell("nobody");
        assert_eq!(
            signals.watched(),
            0,
            "telling an unwatched key makes no entry"
        );
        let a = signals.watch("a".into());
        let again = signals.watch("a".into());
        let b = signals.watch("b".into());
        assert_eq!(signals.watched(), 2);
        {
            let woken = a.notified();
            tokio::pin!(woken);
            woken.as_mut().enable();
            let other = b.notified();
            tokio::pin!(other);
            other.as_mut().enable();
            signals.tell_parts("a", ["s1", "s1", "s2"]);
            woken.await;
            assert!(
                futures_poll(other.as_mut()).is_pending(),
                "a change to a wakes nobody on b"
            );
        }
        assert_eq!((a.count("s1"), again.count("s2"), a.count("s3")), (2, 1, 0));
        drop((a, b));
        assert_eq!(signals.watched(), 1, "a is still watched by another");
        drop(again);
        assert_eq!(signals.watched(), 0);
    }

    fn futures_poll<F: std::future::Future>(
        f: std::pin::Pin<&mut F>,
    ) -> std::task::Poll<F::Output> {
        let waker = std::task::Waker::noop();
        f.poll(&mut std::task::Context::from_waker(waker))
    }
}
