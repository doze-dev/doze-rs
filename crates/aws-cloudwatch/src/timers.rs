//! The alarm timer: one task per region that has a deadline, asleep until it, and gone when there is none.
//!
//! Nothing here polls. The actor works out, after every call, the earliest time any of its alarms needs judging ([`Answer::next_due`]
//! in `core`); the task sleeps until then (or until the deadline moves, which it is told by a watch), asks the actor to judge what
//! is due, and is handed the next deadline. When no alarm can change state without new data, the deadline is `None`, the task
//! ends, and the service holds no timer at all.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use aws_peers::Peers;
use aws_wire::service::Caller;
use doze_runtime::{Ctx, Lazy};
use tokio::sync::watch;

use crate::core::{Core, Request};
use crate::log_eval::{LogOutcome, LogRun};

/// A sleep is cut into pieces no longer than this, so a deadline far ahead does not meet the limit of the timer wheel.
const LONGEST_SLEEP_MS: i64 = 86_400_000;
/// A deadline already past when it is handed back is moved on by this much, so a fault in the arithmetic cannot become a busy loop.
const AT_LEAST_MS: i64 = 250;

/// The timers of every region of one service.
#[derive(Debug)]
pub(crate) struct Timers {
    ctx: Arc<Ctx>,
    regions: Mutex<HashMap<String, watch::Sender<Option<i64>>>>,
    /// The bus a log alarm's query is run through.
    peers: Option<Arc<Peers>>,
}

impl Timers {
    pub(crate) fn new(ctx: Arc<Ctx>, peers: Option<Arc<Peers>>) -> Arc<Self> {
        Arc::new(Self {
            ctx,
            regions: Mutex::new(HashMap::new()),
            peers,
        })
    }

    /// Runs the log alarm queries an answer handed over, each in a task of its own that ends when its answer is judged.
    pub(crate) fn launch(
        self: &Arc<Self>,
        region: &str,
        actor: &Arc<Lazy<Core>>,
        runs: Vec<(Caller, LogRun)>,
    ) {
        for (caller, run) in runs {
            let (timers, actor, region) = (Arc::clone(self), Arc::clone(actor), region.to_owned());
            let stopping = self.ctx.tasks.stopping();
            self.ctx.tasks.spawn(async move {
                tokio::select! {
                    () = stopping.cancelled() => {}
                    () = timers.run_log(region, actor, caller, run) => {}
                }
            });
        }
    }

    async fn run_log(
        self: Arc<Self>,
        region: String,
        actor: Arc<Lazy<Core>>,
        caller: Caller,
        run: LogRun,
    ) {
        let result = match &self.peers {
            Some(peers) => crate::log_query::run(peers, &caller, &run).await,
            // (memory)
            None => Err("CloudWatch Logs is not running in doze-aws.".to_owned()),
        };
        let outcome = Box::new(LogOutcome { run, result });
        if let Ok(mut answer) = actor
            .call(Request::LogResult(caller.clone(), outcome))
            .await
        {
            if let Some(saved) = answer.durable.take() {
                // WHY: a failed save is the writer's to report; the next call finds the same state and says so.
                drop(saved.await);
            }
            self.launch(&region, &actor, std::mem::take(&mut answer.log_runs));
            self.arm(&region, &caller, &actor, answer.next_due);
        }
    }

    /// Tells the region's timer its deadline, starting the task if there is a deadline and no task.
    pub(crate) fn arm(
        self: &Arc<Self>,
        region: &str,
        caller: &Caller,
        actor: &Arc<Lazy<Core>>,
        due: Option<i64>,
    ) {
        let mut map = self.regions.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(tx) = map.get(region) {
            tx.send_replace(due);
            return;
        }
        let Some(at) = due else {
            return;
        };
        let (tx, rx) = watch::channel(Some(at));
        map.insert(region.to_owned(), tx);
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

    fn set(&self, region: &str, due: Option<i64>) {
        let map = self.regions.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(tx) = map.get(region) {
            tx.send_replace(due);
        }
    }

    /// Ends the region's entry if its deadline is still `None`. `false` means a deadline arrived meanwhile and the task goes on.
    fn finish(&self, region: &str) -> bool {
        let mut map = self.regions.lock().unwrap_or_else(PoisonError::into_inner);
        let idle = map.get(region).is_some_and(|tx| tx.borrow().is_none());
        if idle {
            map.remove(region);
        }
        idle
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
        let next = match actor.call(Request::Tick(caller.clone())).await {
            Ok(mut answer) => {
                if let Some(saved) = answer.durable.take() {
                    // WHY: a failed save is the writer's to report; the next call finds the same state and says so.
                    drop(saved.await);
                }
                timers.launch(&region, &actor, std::mem::take(&mut answer.log_runs));
                answer.next_due
            }
            Err(_) => None,
        };
        let now = timers.ctx.clock.now_ms();
        timers.set(&region, next.map(|at| at.max(now + AT_LEAST_MS)));
    }
}
