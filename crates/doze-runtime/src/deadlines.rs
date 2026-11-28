//! A deadline per key (an AWS region) for a [`Lazy`] service: one task asleep until the earliest time the service said it next needs
//! asking, and no task at all while it has nothing due.
//!
//! The service works out its next deadline itself, after every call (SQS: when the oldest message passes its retention; DynamoDB:
//! when the first item's time to live passes) and hands it to [`Deadlines::arm`]. The task sleeps until then (or until the deadline
//! moves, which it is told by a watch), sends the actor the service's tick request, and reads the next deadline from the answer. When
//! the answer is `None` the task ends. Nothing polls. This is the pattern CloudWatch's alarm timer was written to, made generic;
//! [`Due`] is the same with the work of a wake-up left to the service (CloudWatch's alarms, Step Functions' waits).
//!
//! Deadlines arrive from two places, the calls ([`Due::arm`]) and the task's own wake-ups, and the wake-up's answer can be overtaken:
//! a call answered while the wake-up waited on its save arms first, and the wake-up's older "next" would then overwrite it. Every arm
//! moves the key's generation on; a wake-up that finds the generation moved while it worked keeps the earlier of the two deadlines. A
//! deadline too early only costs a wake-up that finds nothing due (the service answers the true next one); a deadline lost or too
//! late was the bug.
//!
//! A nap is never longer than [`LONGEST_NAP`]: the sleep is on the monotonic clock, which stops while the laptop sleeps, so the time
//! is read again (the wall clock, `ctx.clock`) at least that often while something is armed. With nothing armed there is no task.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::watch;

use crate::{Ctx, Lazy, Service};

/// The longest a deadline's task sleeps before it reads the clock again. A sleep stops while the machine sleeps, so a deadline
/// that came due meanwhile is seen at most this long after the machine wakes. Every timer that sleeps toward a wall-clock time uses
/// it (the S3 janitor's midnight too).
pub const LONGEST_NAP: Duration = Duration::from_secs(600);
const LONGEST_NAP_MS: i64 = 600_000;
/// A deadline already past when it is handed back is moved on by this much, so a fault in the arithmetic cannot become a busy loop.
const AT_LEAST_MS: i64 = 250;

type Pending<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// What the service does with the answer to a tick: waits for whatever it must (a save), and says when it is next due.
pub type Settle<S> = Box<dyn Fn(<S as Service>::Response) -> Pending<Option<i64>> + Send + Sync>;

/// What a key's wake-up does: given the key, what it was last armed with and the deadline that came due, does the work and says when
/// the key is next due.
pub type Fire<T> = Box<dyn Fn(String, T, i64) -> Pending<Option<i64>> + Send + Sync>;

/// The deadlines of every key, each woken by a [`Fire`] with what it was armed with (`T`: the caller a tick is made for, say).
pub struct Due<T> {
    ctx: Arc<Ctx>,
    fire: Fire<T>,
    at_least_ms: i64,
    keys: Mutex<HashMap<String, Slot<T>>>,
}

/// One key: the deadline its task sleeps to, how many times it was armed, and what it was last armed with.
struct Slot<T> {
    tx: watch::Sender<Option<i64>>,
    generation: u64,
    with: T,
}

impl<T> std::fmt::Debug for Due<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let keys = self.keys.lock().unwrap_or_else(PoisonError::into_inner);
        f.debug_struct("Due").field("armed", &keys.len()).finish()
    }
}

impl<T: Clone + Send + Sync + 'static> Due<T> {
    /// `fire` does what is due for a key; a deadline it hands back already past is moved on by `at_least` (a floor on how often a
    /// key can wake).
    #[must_use]
    pub fn new(ctx: Arc<Ctx>, at_least: Duration, fire: Fire<T>) -> Arc<Self> {
        Arc::new(Self {
            ctx,
            fire,
            at_least_ms: i64::try_from(at_least.as_millis()).unwrap_or(AT_LEAST_MS),
            keys: Mutex::new(HashMap::new()),
        })
    }

    /// Tells the key's task its deadline (and what to wake with), starting the task if there is a deadline and no task. `None` ends a
    /// running task.
    pub fn arm(self: &Arc<Self>, key: &str, with: &T, due: Option<i64>) {
        let mut map = self.lock();
        if let Some(slot) = map.get_mut(key) {
            slot.generation += 1;
            slot.with = with.clone();
            // A deadline already past wakes the task even when it is the one it sleeps to: the clock moved on under the sleep (the
            // machine slept, or a test moved its clock), and the call that says so should not wait out the nap.
            let now = self.ctx.clock.now_ms();
            slot.tx.send_if_modified(|current| {
                let wake = *current != due || due.is_some_and(|at| at <= now);
                *current = due;
                wake
            });
            return;
        }
        let Some(at) = due else {
            return;
        };
        let (tx, rx) = watch::channel(Some(at));
        let slot = Slot {
            tx,
            generation: 0,
            with: with.clone(),
        };
        map.insert(key.to_owned(), slot);
        self.ctx
            .tasks
            .spawn(run(Arc::clone(self), key.to_owned(), rx));
    }

    /// Whether a key has a task (for tests).
    #[must_use]
    pub fn armed(&self, key: &str) -> bool {
        self.lock().contains_key(key)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Slot<T>>> {
        self.keys.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// What the key was armed with, and its generation, as a wake-up starts.
    fn current(&self, key: &str) -> Option<(T, u64)> {
        self.lock()
            .get(key)
            .map(|slot| (slot.with.clone(), slot.generation))
    }

    /// The deadline a wake-up worked out. Taken as it is when no call armed the key meanwhile (`generation` unchanged); otherwise the
    /// earlier of it and what the call armed, since either may be the newer.
    fn settle(&self, key: &str, next: Option<i64>, generation: u64) {
        let mut map = self.lock();
        let Some(slot) = map.get_mut(key) else {
            return;
        };
        let next = if slot.generation == generation {
            next
        } else {
            match (next, *slot.tx.borrow()) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            }
        };
        // WHY: the floor holds for what a call armed too, or a wake-up that re-arms itself through a call (Step Functions) could loop.
        let floor = self.ctx.clock.now_ms() + self.at_least_ms;
        slot.tx.send_replace(next.map(|at| at.max(floor)));
    }

    /// Ends the key's entry if its deadline is still `None`. `false` means a deadline arrived meanwhile and the task goes on.
    fn finish(&self, key: &str) -> bool {
        let mut map = self.lock();
        let idle = map.get(key).is_some_and(|slot| slot.tx.borrow().is_none());
        if idle {
            map.remove(key);
        }
        idle
    }
}

async fn run<T: Clone + Send + Sync + 'static>(
    due: Arc<Due<T>>,
    key: String,
    mut rx: watch::Receiver<Option<i64>>,
) {
    let stopping = due.ctx.tasks.stopping();
    loop {
        let Some(at) = *rx.borrow_and_update() else {
            if due.finish(&key) {
                return;
            }
            continue;
        };
        let wait = at - due.ctx.clock.now_ms();
        if wait > 0 {
            let nap = Duration::from_millis(u64::try_from(wait.min(LONGEST_NAP_MS)).unwrap_or(0));
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
        let Some((with, generation)) = due.current(&key) else {
            return;
        };
        let next = (due.fire)(key.clone(), with, at).await;
        due.settle(&key, next, generation);
    }
}

/// The deadlines of every key of one service, each woken by sending its actor the tick request.
pub struct Deadlines<S: Service> {
    due: Arc<Due<Arc<Lazy<S>>>>,
}

impl<S: Service> std::fmt::Debug for Deadlines<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Deadlines").field("due", &self.due).finish()
    }
}

impl<S: Service> Deadlines<S> {
    /// `tick` is the request that asks the actor to do what is due; `settle` reads the next deadline (milliseconds since the epoch,
    /// by `ctx.clock`) out of its answer.
    #[must_use]
    pub fn new(
        ctx: Arc<Ctx>,
        tick: impl Fn() -> S::Request + Send + Sync + 'static,
        settle: Settle<S>,
    ) -> Arc<Self> {
        let (tick, settle) = (Arc::new(tick), Arc::new(settle));
        let fire: Fire<Arc<Lazy<S>>> = Box::new(move |_key, actor, _at| {
            let (tick, settle) = (Arc::clone(&tick), Arc::clone(&settle));
            Box::pin(async move {
                match actor.call(tick()).await {
                    Ok(answer) => settle(answer).await,
                    Err(_) => None,
                }
            })
        });
        let due = Due::new(ctx, Duration::from_millis(250), fire);
        Arc::new(Self { due })
    }

    /// Tells the key's task its deadline, starting the task if there is a deadline and no task. `None` ends a running task.
    pub fn arm(self: &Arc<Self>, key: &str, actor: &Arc<Lazy<S>>, due: Option<i64>) {
        self.due.arm(key, actor, due);
    }

    /// Whether a key has a task (for tests).
    #[must_use]
    pub fn armed(&self, key: &str) -> bool {
        self.due.armed(key)
    }
}

#[cfg(test)]
mod tests;
