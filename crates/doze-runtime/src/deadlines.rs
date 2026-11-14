//! A deadline per key (an AWS region) for a [`Lazy`] service: one task asleep until the earliest time the service said it next needs
//! asking, and no task at all while it has nothing due.
//!
//! The service works out its next deadline itself, after every call (SQS: when the oldest message passes its retention; DynamoDB:
//! when the first item's time to live passes) and hands it to [`Deadlines::arm`]. The task sleeps until then (or until the deadline
//! moves, which it is told by a watch), sends the actor the service's tick request, and reads the next deadline from the answer. When
//! the answer is `None` the task ends. Nothing polls. This is the pattern CloudWatch's alarm timer was written to, made generic.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::watch;

use crate::{Ctx, Lazy, Service};

/// A sleep is cut into pieces no longer than this, so a deadline far ahead does not meet the limit of the timer wheel.
const LONGEST_SLEEP_MS: i64 = 86_400_000;
/// A deadline already past when it is handed back is moved on by this much, so a fault in the arithmetic cannot become a busy loop.
const AT_LEAST_MS: i64 = 250;

/// What the service does with the answer to a tick: waits for whatever it must (a save), and says when it is next due.
pub type Settle<S> = Box<
    dyn Fn(<S as Service>::Response) -> Pin<Box<dyn Future<Output = Option<i64>> + Send>>
        + Send
        + Sync,
>;

/// The deadlines of every key of one service.
pub struct Deadlines<S: Service> {
    ctx: Arc<Ctx>,
    tick: Box<dyn Fn() -> S::Request + Send + Sync>,
    settle: Settle<S>,
    keys: Mutex<HashMap<String, watch::Sender<Option<i64>>>>,
}

impl<S: Service> std::fmt::Debug for Deadlines<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let keys = self.keys.lock().unwrap_or_else(PoisonError::into_inner);
        f.debug_struct("Deadlines")
            .field("armed", &keys.len())
            .finish()
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
        Arc::new(Self {
            ctx,
            tick: Box::new(tick),
            settle,
            keys: Mutex::new(HashMap::new()),
        })
    }

    /// Tells the key's task its deadline, starting the task if there is a deadline and no task. `None` ends a running task.
    pub fn arm(self: &Arc<Self>, key: &str, actor: &Arc<Lazy<S>>, due: Option<i64>) {
        let mut map = self.keys.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(tx) = map.get(key) {
            tx.send_if_modified(|now| {
                let changed = *now != due;
                *now = due;
                changed
            });
            return;
        }
        let Some(at) = due else {
            return;
        };
        let (tx, rx) = watch::channel(Some(at));
        map.insert(key.to_owned(), tx);
        self.ctx
            .tasks
            .spawn(run(Arc::clone(self), key.to_owned(), Arc::clone(actor), rx));
    }

    /// Whether a key has a task (for tests).
    #[must_use]
    pub fn armed(&self, key: &str) -> bool {
        self.keys
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(key)
    }

    fn set(&self, key: &str, due: Option<i64>) {
        let map = self.keys.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(tx) = map.get(key) {
            tx.send_replace(due);
        }
    }

    /// Ends the key's entry if its deadline is still `None`. `false` means a deadline arrived meanwhile and the task goes on.
    fn finish(&self, key: &str) -> bool {
        let mut map = self.keys.lock().unwrap_or_else(PoisonError::into_inner);
        let idle = map.get(key).is_some_and(|tx| tx.borrow().is_none());
        if idle {
            map.remove(key);
        }
        idle
    }
}

async fn run<S: Service>(
    deadlines: Arc<Deadlines<S>>,
    key: String,
    actor: Arc<Lazy<S>>,
    mut rx: watch::Receiver<Option<i64>>,
) {
    let stopping = deadlines.ctx.tasks.stopping();
    loop {
        let due = *rx.borrow_and_update();
        let Some(at) = due else {
            if deadlines.finish(&key) {
                return;
            }
            continue;
        };
        let wait = at - deadlines.ctx.clock.now_ms();
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
        let next = match actor.call((deadlines.tick)()).await {
            Ok(answer) => (deadlines.settle)(answer).await,
            Err(_) => None,
        };
        let now = deadlines.ctx.clock.now_ms();
        deadlines.set(&key, next.map(|at| at.max(now + AT_LEAST_MS)));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::{ManualClock, Tasks};

    /// Counts its ticks; each tick is due again `again` ms later, until it has ticked `times` times.
    struct Ticker {
        ticks: Arc<AtomicUsize>,
    }

    impl Service for Ticker {
        type Request = ();
        type Response = usize;
        type Error = std::convert::Infallible;

        async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, Self::Error> {
            let ticks = ctx
                .ext
                .as_ref()
                .and_then(|e| Arc::clone(e).downcast::<AtomicUsize>().ok())
                .unwrap_or_default();
            Ok(Self { ticks })
        }

        async fn handle(&mut self, (): ()) -> usize {
            self.ticks.fetch_add(1, Ordering::SeqCst) + 1
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_deadline_ticks_once_and_the_task_ends_when_nothing_is_due() {
        let ticks = Arc::new(AtomicUsize::new(0));
        let clock = Arc::new(ManualClock::at(1_000));
        let tasks = Tasks::new();
        let ctx = Arc::new(Ctx {
            dir: std::env::temp_dir(),
            idle: Duration::from_secs(60),
            tasks: tasks.clone(),
            clock: clock.clone(),
            ext: Some(ticks.clone()),
        });
        let actor = Arc::new(Lazy::<Ticker>::new("ticker", Arc::clone(&ctx)));
        let deadlines = Deadlines::<Ticker>::new(
            ctx,
            || (),
            Box::new(|n| Box::pin(async move { (n < 2).then_some(0) })),
        );
        deadlines.arm("r", &actor, None);
        assert!(!deadlines.armed("r"), "nothing due: no task");
        deadlines.arm("r", &actor, Some(5_000));
        assert!(deadlines.armed("r"));
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(ticks.load(Ordering::SeqCst), 0, "not due yet");
        clock.set(5_000);
        // The task naps in pieces of at most the deadline's distance: advance past the original four seconds.
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert_eq!(ticks.load(Ordering::SeqCst), 1, "due");
        // The answer said "due again at once": the task waits the floor, by the service's clock.
        clock.set(6_000);
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(ticks.load(Ordering::SeqCst), 2, "due again once");
        assert!(
            !deadlines.armed("r"),
            "nothing due after the second: the task ended"
        );
        tasks.shutdown().await;
    }
}
