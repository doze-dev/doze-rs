//! A publish's deliveries, run side by side. Each subscription's delivery is its own (its queue, its function, its dead-letter
//! queue), so nothing orders one before another; awaited one after another, ten SQS subscriptions were ten durable commits in a
//! row (about 70 ms a publish). Awaited together, their writes meet in the store's group commit.
//!
//! No task is spawned: the deliveries are polled in the publishing actor's own task, at most [`AT_ONCE`] in flight, the next starting
//! as one ends. A subscription still gets a topic's messages in order: the actor finishes one publish's deliveries before it takes the
//! next call, and a batch publishes its messages one after another.

use std::future::Future;
use std::pin::Pin;
use std::task::Poll;

/// The most deliveries of one publish in flight at once.
pub(crate) const AT_ONCE: usize = 16;

/// Runs every future of `work` to its end, at most [`AT_ONCE`] at a time.
pub(crate) async fn all<F: Future<Output = ()>>(work: impl IntoIterator<Item = F>) {
    let mut waiting = work.into_iter();
    let mut running: Vec<Pin<Box<F>>> = waiting.by_ref().take(AT_ONCE).map(Box::pin).collect();
    std::future::poll_fn(|cx| {
        let mut i = 0;
        while let Some(f) = running.get_mut(i) {
            if f.as_mut().poll(cx).is_ready() {
                // The last one takes this place and is polled next; a waiting one joins at the end.
                drop(running.swap_remove(i));
                running.extend(waiting.next().map(Box::pin));
            } else {
                i += 1;
            }
        }
        if running.is_empty() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use std::cell::{Cell, RefCell};
    use std::time::Duration;

    use super::*;

    /// Deliveries overlap (forty that each wait 50 ms end in about 150 ms, not two seconds), never more than [`AT_ONCE`] at once.
    #[tokio::test(start_paused = true)]
    async fn deliveries_run_together_and_no_more_than_the_cap_at_once() {
        let (now, most, done) = (Cell::new(0), Cell::new(0), RefCell::new(Vec::new()));
        let began = tokio::time::Instant::now();
        all((0..40).map(|n| {
            let (now, most, done) = (&now, &most, &done);
            async move {
                now.set(now.get() + 1);
                most.set(most.get().max(now.get()));
                tokio::time::sleep(Duration::from_millis(50)).await;
                now.set(now.get() - 1);
                done.borrow_mut().push(n);
            }
        }))
        .await;
        assert_eq!(most.get(), AT_ONCE);
        assert_eq!(done.borrow().len(), 40);
        assert_eq!(began.elapsed(), Duration::from_millis(150));
        all(std::iter::empty::<std::future::Ready<()>>()).await;
    }
}
