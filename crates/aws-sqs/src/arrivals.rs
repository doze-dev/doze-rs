//! Who waits on which queue. A receive that waits (a long poll, or a Lambda event source mapping between reads) watches its
//! queue, and is woken when a call there adds a message or makes one show sooner; a call on another queue does not wake it. The
//! watch also keeps when the queue's first hidden message shows, as the last receive that found nothing was told, so a waiter
//! sleeps until then and not a moment less. An entry exists only while someone watches: a queue nobody waits on costs nothing.
//! The wake-ups are doze-runtime's [`Signals`], with that time as the value each queue's entry carries.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use doze_runtime::{Clock, Signal, Signals};
use tokio::sync::futures::Notified;

/// The watched queues, by `region/name`.
pub(crate) struct Arrivals {
    clock: Arc<dyn Clock>,
    signals: Arc<Signals<NextVisible>>,
}

impl std::fmt::Debug for Arrivals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Arrivals").finish_non_exhaustive()
    }
}

/// When the queue's first hidden message shows, in the clock's milliseconds; `i64::MAX` when none is hidden or it is not known yet.
#[derive(Debug)]
pub(crate) struct NextVisible(AtomicI64);

impl Default for NextVisible {
    fn default() -> Self {
        Self(AtomicI64::new(i64::MAX))
    }
}

/// The key of a queue: queues are kept per region, by name.
pub(crate) fn key(region: &str, name: &str) -> String {
    format!("{region}/{name}")
}

impl Arrivals {
    pub(crate) fn new(clock: Arc<dyn Clock>) -> Arc<Self> {
        Arc::new(Self {
            clock,
            signals: Arc::default(),
        })
    }

    /// Starts watching the queue `key` names.
    pub(crate) fn watch(&self, key: String) -> Watch {
        Watch {
            clock: Arc::clone(&self.clock),
            signal: self.signals.watch(key),
        }
    }

    /// Wakes whoever waits on the queue `key` names; nothing when nobody does.
    pub(crate) fn tell(&self, key: &str) {
        self.signals
            .with_extra(key, |next| next.0.store(i64::MAX, Ordering::Relaxed));
        self.signals.tell(key);
    }

    /// Notes what a receive that found nothing was told about when the queue's first hidden message shows.
    pub(crate) fn note(&self, key: &str, next_visible_ms: Option<i64>) {
        self.signals.with_extra(key, |next| {
            next.0
                .store(next_visible_ms.unwrap_or(i64::MAX), Ordering::Relaxed);
        });
    }

    /// How many queues are watched (for tests).
    #[cfg(test)]
    pub(crate) fn watched(&self) -> usize {
        self.signals.watched()
    }
}

/// A watch on one queue, ended when dropped.
#[derive(Debug)]
pub struct Watch {
    clock: Arc<dyn Clock>,
    signal: Signal<NextVisible>,
}

impl Watch {
    /// A wake-up for the next arrival. Call `enable` on it (pinned) before reading the queue, so an arrival between the read and
    /// the wait is not missed.
    pub fn notified(&self) -> Notified<'_> {
        self.signal.notified()
    }

    /// How long until the queue's first hidden message shows (in flight or delayed), as the last receive that found nothing was
    /// told; `None` when none is hidden, so only an arrival can give a waiter something.
    #[must_use]
    pub fn until_visible(&self) -> Option<Duration> {
        let at = self.signal.extra().0.load(Ordering::Relaxed);
        if at == i64::MAX {
            return None;
        }
        let ms = at.saturating_sub(self.clock.now_ms()).max(1);
        Some(Duration::from_millis(u64::try_from(ms).unwrap_or(1)))
    }
}
