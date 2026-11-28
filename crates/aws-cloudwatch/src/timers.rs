//! The alarm timer: one task per region that has a deadline, asleep until it, and gone when there is none.
//!
//! Nothing here polls. The actor works out, after every call, the earliest time any of its alarms needs judging ([`Answer::next_due`]
//! in `core`); the region's deadline (doze-runtime's [`Due`], which keeps a deadline a call arms while a wake-up is saving) sleeps
//! until then, asks the actor to judge what is due, starts the log alarms' queries the answer hands over, and takes the next deadline.
//! When no alarm can change state without new data, the deadline is `None`, the task ends, and the service holds no timer at all.

use std::sync::{Arc, Weak};
use std::time::Duration;

use aws_peers::Peers;
use aws_wire::service::Caller;
use doze_runtime::{Ctx, Due, Fire, Lazy};

use crate::core::{Core, Request};
use crate::log_eval::{LogOutcome, LogRun};

/// A deadline already past when it is handed back is moved on by this much, so a fault in the arithmetic cannot become a busy loop.
const AT_LEAST: Duration = Duration::from_millis(250);

/// What a region's wake-up asks with: the caller its tick is made for, and the region's actor.
type Armed = (Caller, Arc<Lazy<Core>>);

/// The timers of every region of one service.
#[derive(Debug)]
pub(crate) struct Timers {
    ctx: Arc<Ctx>,
    due: Arc<Due<Armed>>,
    /// The bus a log alarm's query is run through.
    peers: Option<Arc<Peers>>,
}

impl Timers {
    pub(crate) fn new(ctx: Arc<Ctx>, peers: Option<Arc<Peers>>) -> Arc<Self> {
        Arc::new_cyclic(|me: &Weak<Self>| {
            let me = me.clone();
            let fire: Fire<Armed> = Box::new(move |region, (caller, actor), _at| {
                let me = me.clone();
                Box::pin(async move {
                    let mut answer = actor.call(Request::Tick(caller)).await.ok()?;
                    if let Some(saved) = answer.durable.take() {
                        // WHY: a failed save is the writer's to report; the next call finds the same state and says so.
                        drop(saved.await);
                    }
                    if let Some(timers) = me.upgrade() {
                        timers.launch(&region, &actor, std::mem::take(&mut answer.log_runs));
                    }
                    answer.next_due
                })
            });
            Self {
                due: Due::new(Arc::clone(&ctx), AT_LEAST, fire),
                ctx,
                peers,
            }
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
        if let Ok((mut answer, number)) = actor
            .call_numbered(Request::LogResult(caller.clone(), outcome))
            .await
        {
            if let Some(saved) = answer.durable.take() {
                // WHY: a failed save is the writer's to report; the next call finds the same state and says so.
                drop(saved.await);
            }
            self.launch(&region, &actor, std::mem::take(&mut answer.log_runs));
            self.arm(&region, &caller, &actor, (answer.next_due, number));
        }
    }

    /// Tells the region's timer the deadline of the answer numbered `number` ([`Lazy::call_numbered`]), starting the task if there is a
    /// deadline and no task. An answer older than one already armed from is dropped: a call that awaited its save is overtaken.
    pub(crate) fn arm(
        self: &Arc<Self>,
        region: &str,
        caller: &Caller,
        actor: &Arc<Lazy<Core>>,
        (due, number): (Option<i64>, u64),
    ) {
        self.due
            .arm_ordered(region, &(caller.clone(), Arc::clone(actor)), due, number);
    }

    /// Whether a region has a timer task (for tests).
    #[cfg(test)]
    pub(crate) fn armed(&self, region: &str) -> bool {
        self.due.armed(region)
    }
}
