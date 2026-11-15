//! The schedule timer: one task per region that has a deadline, asleep until it, and gone when there is none.
//!
//! Nothing here polls. The actor works out, after every call, the earliest time a scheduled rule or a retry is due
//! ([`Answer::next_due`](crate::core::Answer)); the task sleeps until then (or until the deadline moves, which a watch tells it),
//! asks the actor to do what is due, and is handed the next deadline. With nothing due, the deadline is `None`, the task ends, and the
//! service holds no timer at all.
//!
//! Deadlines arrive from two places, the calls and the timer's own wake-ups, and may arrive out of order (a call answered while a
//! wake-up was in the actor's queue). Each answer carries the order the actor gave it in (`generation`), and the timer keeps the
//! deadline of the latest answer, so an accepted event is never left waiting behind a stale "nothing due".

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use aws_wire::service::Caller;
use doze_runtime::{Ctx, Lazy};
use tokio::sync::watch;

use crate::core::{Core, Request};

/// A sleep is cut into pieces no longer than this, so a deadline far ahead does not meet the limit of the timer wheel.
const LONGEST_SLEEP_MS: i64 = 86_400_000;
/// A deadline already past when it is handed back is moved on by this much, so a fault in the arithmetic cannot become a busy loop.
const AT_LEAST_MS: i64 = 250;

/// The timers of every region of one service.
#[derive(Debug)]
pub(crate) struct Timers {
    ctx: Arc<Ctx>,
    regions: Mutex<HashMap<String, Slot>>,
}

/// One region's timer: the deadline it sleeps to, and the generation of the answer that set it.
#[derive(Debug)]
struct Slot {
    generation: u64,
    tx: watch::Sender<Option<i64>>,
}

impl Timers {
    pub(crate) fn new(ctx: Arc<Ctx>) -> Arc<Self> {
        Arc::new(Self {
            ctx,
            regions: Mutex::new(HashMap::new()),
        })
    }

    /// Tells the region's timer the deadline of answer `generation`, starting the task if there is a deadline and no task.
    pub(crate) fn arm(
        self: &Arc<Self>,
        region: &str,
        caller: &Caller,
        actor: &Arc<Lazy<Core>>,
        (due, generation): (Option<i64>, u64),
    ) {
        let mut map = self.regions.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(slot) = map.get_mut(region) {
            slot.update(due, generation);
            return;
        }
        let Some(at) = due else {
            return;
        };
        let (tx, rx) = watch::channel(Some(at));
        map.insert(region.to_owned(), Slot { generation, tx });
        self.ctx.tasks.spawn(run(
            Arc::clone(self),
            region.to_owned(),
            caller.clone(),
            Arc::clone(actor),
            rx,
        ));
    }

    /// Whether a region has a timer task (for tests).
    #[cfg(test)]
    pub(crate) fn armed(&self, region: &str) -> bool {
        self.regions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(region)
    }

    fn set(&self, region: &str, due: Option<i64>, generation: u64) {
        let mut map = self.regions.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(slot) = map.get_mut(region) {
            slot.update(due, generation);
        }
    }

    /// Ends the region's entry if its deadline is still `None`. `false` means a deadline arrived meanwhile and the task goes on.
    fn finish(&self, region: &str) -> bool {
        let mut map = self.regions.lock().unwrap_or_else(PoisonError::into_inner);
        let idle = map
            .get(region)
            .is_some_and(|slot| slot.tx.borrow().is_none());
        if idle {
            map.remove(region);
        }
        idle
    }
}

impl Slot {
    /// Takes the deadline unless it comes from an answer older than the one already here.
    fn update(&mut self, due: Option<i64>, generation: u64) {
        if generation >= self.generation {
            self.generation = generation;
            self.tx.send_replace(due);
        }
    }
}

async fn run(
    timers: Arc<Timers>,
    region: String,
    caller: Caller,
    actor: Arc<Lazy<Core>>,
    mut rx: watch::Receiver<Option<i64>>,
) {
    let stopping = timers.ctx.tasks.stopping();
    loop {
        let due = *rx.borrow_and_update();
        let Some(at) = due else {
            if timers.finish(&region) {
                return;
            }
            continue;
        };
        let wait = at - timers.ctx.clock.now_ms();
        if wait > 0 {
            let nap = Duration::from_millis(u64::try_from(wait.min(LONGEST_SLEEP_MS)).unwrap_or(0));
            tokio::select! {
                () = tokio::time::sleep(nap) => {}
                changed = rx.changed() => {
                    if changed.is_err() {
                        return;
                    }
                }
                () = stopping.cancelled() => return,
            }
            continue;
        }
        let (next, generation) = match actor.call(Request::Tick(caller.clone(), at)).await {
            Ok(mut answer) => {
                if let Some(saved) = answer.durable.take() {
                    // WHY: a failed save is the writer's to report; the next call finds the same state and says so.
                    drop(saved.await);
                }
                (answer.next_due, answer.generation)
            }
            Err(_) => (None, 0),
        };
        let now = timers.ctx.clock.now_ms();
        timers.set(
            &region,
            next.map(|at| at.max(now + AT_LEAST_MS)),
            generation,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_deadline_from_an_older_answer_does_not_replace_a_newer_one() {
        let (tx, rx) = watch::channel(Some(10));
        let mut slot = Slot { generation: 5, tx };
        // A wake-up answered before the call that accepted an event comes back late saying nothing is due: it is ignored.
        slot.update(None, 4);
        assert_eq!(*rx.borrow(), Some(10));
        slot.update(Some(20), 6);
        assert_eq!(*rx.borrow(), Some(20));
        slot.update(None, 7);
        assert_eq!(*rx.borrow(), None);
    }
}
