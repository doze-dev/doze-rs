//! The one timer mechanism: a set of deadlines that costs nothing while it is empty.
//!
//! Wraps tokio-util's `DelayQueue` (O(1) insert and remove by key, one timer for any number of
//! deadlines). The wrapper exists for three traps the raw queue sets (spike C):
//! - an *empty* `DelayQueue` completes `next()` at once with `None`, so a `select!` loop that polls
//!   it spins at full CPU; [`Expiring::expired`] turns "empty" into "pending forever";
//! - `DelayQueue::remove` panics on a key that already fired (and with `panic = "abort"` that kills the
//!   process); [`Expiring::remove`] answers `None`;
//! - a deadline past the wheel (about two years) panics on insert; it is clamped to the wheel's edge.

use std::future;
use std::task::Poll;
use std::time::Duration;

use tokio_util::time::DelayQueue;
use tokio_util::time::delay_queue::{Expired, Key};

/// The wheel covers 2^36 ms (about 2.18 years); the margin keeps a clamped deadline inside it.
const MAX_AFTER: Duration = Duration::from_millis((1 << 36) - 1 - 1000);

/// A set of deadlines owned by one actor. Wrap the actor's `select!` around [`Self::expired`].
#[derive(Debug)]
pub struct Expiring<T> {
    queue: DelayQueue<T>,
}

impl<T> Default for Expiring<T> {
    fn default() -> Self {
        Self {
            queue: DelayQueue::new(),
        }
    }
}

impl<T> Expiring<T> {
    /// Starts a deadline `after` from now. One beyond the wheel is clamped to its edge.
    pub fn insert(&mut self, value: T, after: Duration) -> Key {
        self.queue.insert(value, after.min(MAX_AFTER))
    }

    /// Cancels a deadline. `None` if it already fired or was removed.
    pub fn remove(&mut self, key: &Key) -> Option<T> {
        self.queue.try_remove(key).map(Expired::into_inner)
    }

    /// Moves a deadline to `after` from now and returns its new key (the old one is spent). `None` if
    /// it already fired or was removed. (`DelayQueue::reset` panics on such a key, so this removes and
    /// inserts again.)
    pub fn reset(&mut self, key: &Key, after: Duration) -> Option<Key> {
        let value = self.queue.try_remove(key)?.into_inner();
        Some(self.insert(value, after))
    }

    /// Gives the slab's memory back. The queue never shrinks by itself: after a burst of 100,000
    /// entries it keeps their slots until this is called.
    pub fn compact(&mut self) {
        self.queue.compact();
    }

    /// How many deadlines are pending.
    #[must_use]
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// No deadline is pending.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Resolves with the next value whose deadline has passed. While the set is empty it is pending
    /// without registering a timer, so an idle actor costs no wake-ups; `insert` wakes the task stored by
    /// the last poll.
    pub async fn expired(&mut self) -> T {
        future::poll_fn(|cx| match self.queue.poll_expired(cx) {
            Poll::Ready(Some(item)) => Poll::Ready(item.into_inner()),
            Poll::Ready(None) | Poll::Pending => Poll::Pending,
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use tokio::time::{Instant, advance, timeout};

    use super::*;

    #[tokio::test(start_paused = true)]
    async fn deadlines_fire_in_order_and_a_second_remove_is_not_a_panic() {
        let mut q = Expiring::default();
        q.insert("late", Duration::from_secs(20));
        let early = q.insert("early", Duration::from_secs(10));
        let doomed = q.insert("doomed", Duration::from_secs(5));
        assert_eq!(q.remove(&doomed), Some("doomed"));
        assert_eq!(q.remove(&doomed), None);
        assert!(timeout(Duration::from_secs(9), q.expired()).await.is_err());
        advance(Duration::from_secs(1)).await;
        assert_eq!(q.expired().await, "early");
        assert_eq!(q.remove(&early), None, "already fired");
    }

    #[tokio::test(start_paused = true)]
    async fn a_deadline_beyond_the_wheel_is_clamped_not_a_panic() {
        let mut q = Expiring::default();
        q.insert(1u8, Duration::from_secs(60 * 60 * 24 * 365 * 10));
        assert_eq!(q.len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn an_empty_set_registers_no_timer_and_wakes_on_insert() {
        let mut q = Expiring::<u8>::default();
        // With auto-advance, a pending future with no timers of its own makes the runtime jump to the
        // timeout: if `expired` registered one, the hour would pass in fewer steps. It must stay pending.
        assert!(
            timeout(Duration::from_secs(3600), q.expired())
                .await
                .is_err()
        );
        q.insert(7, Duration::from_secs(1));
        assert_eq!(q.expired().await, 7);
        assert!(q.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn an_insert_while_waiting_re_arms_for_an_earlier_deadline() {
        let mut q = Expiring::default();
        q.insert(1u8, Duration::from_secs(100));
        let t0 = Instant::now();
        let got = timeout(Duration::from_secs(5), q.expired()).await;
        assert!(got.is_err());
        q.insert(2, Duration::from_secs(1));
        assert_eq!(q.expired().await, 2);
        assert!(t0.elapsed() < Duration::from_secs(10));
    }

    #[tokio::test(start_paused = true)]
    async fn reset_moves_a_deadline_and_ignores_one_that_fired() {
        let mut q = Expiring::default();
        let key = q.insert("a", Duration::from_secs(10));
        let key = q.reset(&key, Duration::from_secs(1)).unwrap();
        assert_eq!(q.expired().await, "a");
        assert!(q.reset(&key, Duration::from_secs(1)).is_none());
        assert!(q.is_empty());
    }
}
