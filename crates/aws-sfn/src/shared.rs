//! What lives beside the actors and outlives an idle reap: the actors themselves, the regions' timers, the `StartSyncExecution`
//! callers waiting for an execution to end, the `GetActivityTask` pollers, and the role sessions tasks run as. Every answer of an
//! actor comes through [`Shared::ask`], which waits for it to be on disk and then does what it asks: task calls in tasks of their
//! own, wake-ups for pollers and waiters, log records and status events.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use aws_peers::Peers;
use aws_wire::ApiError;
use aws_wire::service::Caller;
use doze_runtime::{Ctx, Keyed};
use serde_json::Value;
use tokio::sync::{Notify, oneshot};

use crate::core::{Answer, Asked, Core, Request};
use crate::effects::Effects;
use crate::role::Sessions;
use crate::timers::Timers;

/// The region whose files are kept in the service's directory itself.
pub(crate) const HOME_REGION: &str = "us-east-1";
/// How many ended Express executions nobody waited for are remembered for a `StartSyncExecution` that is about to.
const MOST_UNCLAIMED: usize = 256;

/// A `StartSyncExecution` waiting for its execution, or the answer that came before it waited.
#[derive(Debug)]
enum Slot {
    Waiting(oneshot::Sender<Value>),
    Done(Value),
}

/// Everything the regions share.
#[derive(Debug)]
pub(crate) struct Shared {
    pub(crate) ctx: Arc<Ctx>,
    pub(crate) actors: Keyed<Core>,
    pub(crate) timers: Arc<Timers>,
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) sessions: Sessions,
    pub(crate) config: Arc<crate::Config>,
    sync: Mutex<HashMap<String, Slot>>,
    polls: Mutex<HashMap<String, Arc<Notify>>>,
}

impl Shared {
    pub(crate) fn new(ctx: Arc<Ctx>, config: crate::Config) -> Arc<Self> {
        let peers = ctx
            .ext
            .as_ref()
            .and_then(|e| Arc::clone(e).downcast::<Peers>().ok());
        Arc::new(Self {
            timers: Timers::new(Arc::clone(&ctx)),
            actors: Keyed::new("states", Arc::clone(&ctx), HOME_REGION),
            ctx,
            peers,
            sessions: Sessions::default(),
            config: Arc::new(config),
            sync: Mutex::new(HashMap::new()),
            polls: Mutex::new(HashMap::new()),
        })
    }

    /// Asks the actor of `region`, waits until what it changed is on disk, does what the answer asks, and re-arms the timer.
    pub(crate) async fn ask(
        self: &Arc<Self>,
        region: &str,
        caller: &Caller,
        req: Request,
    ) -> Result<Answer, ApiError> {
        let actor = self
            .actors
            .actor(region)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let mut answer = actor
            .call(Asked {
                config: Arc::clone(&self.config),
                req,
            })
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        if let Some(saved) = answer.durable.take()
            && let Err(e) = saved.await
        {
            return Err(ApiError::internal(format!("could not save: {e}")));
        }
        let effects = std::mem::take(&mut answer.effects);
        answer.effects.sync = effects.sync.clone();
        answer.effects.taken = effects.taken;
        self.carry_out(caller, effects);
        self.timers.arm(self, region, caller, answer.next_due);
        Ok(answer)
    }

    fn carry_out(self: &Arc<Self>, caller: &Caller, effects: Effects) {
        for ended in effects.ended {
            self.ended(ended.arn, ended.answer);
        }
        for activity in effects.activities {
            self.poll_slot(&activity).notify_waiters();
        }
        for job in effects.jobs {
            let (shared, caller) = (Arc::clone(self), caller.clone());
            self.ctx
                .tasks
                .spawn(async move { crate::integ::run(shared, caller, job).await });
        }
        if let Some(peers) = self.peers.clone() {
            let (logs, notices, caller) = (effects.logs, effects.notices, caller.clone());
            if !logs.is_empty() || !notices.is_empty() {
                self.ctx
                    .tasks
                    .spawn(async move { crate::deliver::send(peers, caller, logs, notices).await });
            }
        }
    }

    fn ended(&self, arn: String, answer: Value) {
        let mut sync = self.sync.lock().unwrap_or_else(PoisonError::into_inner);
        match sync.remove(&arn) {
            Some(Slot::Waiting(tx)) => {
                // WHY: a caller that went away no longer needs the answer.
                drop(tx.send(answer));
            }
            _ if arn.contains(":express:") => {
                if sync.len() >= MOST_UNCLAIMED {
                    sync.retain(|_, s| matches!(s, Slot::Waiting(_)));
                }
                sync.insert(arn, Slot::Done(answer));
            }
            _ => {}
        }
    }

    /// Waits for an Express execution to end (or takes the answer that already came).
    pub(crate) fn wait_for(&self, arn: &str) -> oneshot::Receiver<Value> {
        let (tx, rx) = oneshot::channel();
        let mut sync = self.sync.lock().unwrap_or_else(PoisonError::into_inner);
        match sync.remove(arn) {
            Some(Slot::Done(answer)) => {
                drop(tx.send(answer));
            }
            _ => {
                sync.insert(arn.to_owned(), Slot::Waiting(tx));
            }
        }
        rx
    }

    /// The wake-up of an activity's pollers.
    pub(crate) fn poll_slot(&self, activity: &str) -> Arc<Notify> {
        let mut polls = self.polls.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(polls.entry(activity.to_owned()).or_default())
    }
}
