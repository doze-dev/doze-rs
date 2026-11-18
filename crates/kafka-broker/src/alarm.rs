//! When a background task next has something to do on its own, so that it sleeps until then and no longer.
//!
//! The transaction coordinator's: abort a transaction that outlives its timeout, try again a decision whose markers could
//! not be written, forget an id that has been idle for the expiry. It used to look every half second whatever there was,
//! which was all of an idle broker's wake-ups (about four a second, measured); with no transactional id at all it does not
//! wake. Every change to a transaction ([`Broker::update_txn`](crate::Broker)) sets the alarm to that transaction's next
//! step if that is sooner than the time set; a look over every transaction, when the alarm goes, sets it to the soonest.
//!
//! The share groups' (`crate::share`): the soonest an acquisition lock runs out. Set by every acquisition and renewal; with
//! no record acquired it does not wake.
//!
//! Early is harmless (a look finds nothing to do and sets it again); late is what this exists to prevent, so a time is only
//! ever moved earlier between looks.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use tokio::sync::Notify;

/// The time of the next look, in milliseconds since the epoch (`i64::MAX`: none), and the bell that says it moved.
#[derive(Debug)]
pub(crate) struct Alarm {
    next_ms: AtomicI64,
    bell: Arc<Notify>,
}

impl Default for Alarm {
    fn default() -> Self {
        Self {
            // Due at once: the first look finds what the store held at the start and sets the alarm from it.
            next_ms: AtomicI64::new(i64::MIN),
            bell: Arc::new(Notify::new()),
        }
    }
}

impl Alarm {
    /// An alarm with nothing set: its task waits for the first time to be set (no look at the start).
    #[must_use]
    pub(crate) fn unset() -> Self {
        let alarm = Self::default();
        alarm.clear();
        alarm
    }

    /// Asks for a look at `at_ms`, or sooner if one is set sooner.
    pub(crate) fn set(&self, at_ms: i64) {
        if at_ms < self.next_ms.fetch_min(at_ms, Ordering::AcqRel) {
            // Stored if the task is not waiting yet, so a ring between its reading the time and its waiting is not lost.
            self.bell.notify_one();
        }
    }

    /// When the next look is due (`i64::MAX`: none).
    pub(crate) fn next_ms(&self) -> i64 {
        self.next_ms.load(Ordering::Acquire)
    }

    /// Takes the alarm off before a look: the look sets it again from what it finds, and a change made while it looks sets
    /// it too.
    pub(crate) fn clear(&self) {
        self.next_ms.store(i64::MAX, Ordering::Release);
    }

    /// The bell, for the task to wait on without holding the broker.
    pub(crate) fn bell(&self) -> Arc<Notify> {
        Arc::clone(&self.bell)
    }
}

impl Drop for Alarm {
    /// The broker is going: the task, waiting with nothing set, wakes to find it gone and ends.
    fn drop(&mut self) {
        self.bell.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_alarm_only_moves_earlier_and_rings_when_it_does() {
        let a = Alarm::default();
        a.clear();
        assert_eq!(a.next_ms(), i64::MAX);
        a.set(500);
        assert_eq!(a.next_ms(), 500);
        a.set(900);
        assert_eq!(a.next_ms(), 500, "a later time does not move it");
        a.set(100);
        assert_eq!(a.next_ms(), 100);
        // Rung (a permit is stored): the first wait returns at once, and only one is stored.
        let bell = a.bell();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        rt.block_on(async {
            tokio::time::timeout(std::time::Duration::from_millis(50), bell.notified())
                .await
                .expect("rung");
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(50), bell.notified())
                    .await
                    .is_err(),
                "one permit"
            );
        });
    }
}
