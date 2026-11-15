//! EventBridge Scheduler: schedules and schedule groups as a state machine behind a lazy actor per region, with `rate()`, `cron()`
//! and `at()` read in any IANA time zone (`aws-cron`), and invocations of templated and universal targets made over the peer bus as
//! the schedule's execution role, with the target's retry policy and dead-letter queue. State is on disk and survives an idle reap
//! and a restart. The Go edition had no Scheduler.
//!
//! **Deadline-driven, not ticked.** Every schedule's next fire is on its record; after every call the region's earliest is handed to
//! one timer, which sleeps to it, asks the actor to fire what is due, and is handed the next. With nothing scheduled there is no
//! timer and no task. [`Scheduler::resume`] arms the regions that have schedules when the process starts.
//!
//! **Local hook:** `POST /_doze/scheduler/<region>/<group>/<name>/fire` (optionally `?account=<id>`) invokes a schedule now and
//! answers what the first attempt came to; see `hook`.

mod authz;
mod check;
mod core;
mod deliver;
mod fire;
mod groups;
mod hook;
mod model;
mod page;
mod plan;
mod req;
mod retry;
mod role;
mod schedules;
mod tags;
mod targets;
mod universal;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aws_model_scheduler::Operation;
use aws_model_scheduler::meta::META;
use aws_model_scheduler::tables::TABLES;
use aws_peers::Peers;
use aws_schema::meta::ServiceMeta;
use aws_schema::patterns::Patterns;
use aws_schema::tables::Tables;
use aws_wire::ApiError;
use aws_wire::handler::reply_json;
use aws_wire::rest::{RestReply, RestService, Upload};
use aws_wire::service::{BoxFuture, Call, Reply, Service, Web, WebReply, WebRequest};
use doze_runtime::{Clock, Ctx, Deadlines, Keyed, Lazy, Tasks};
use serde_json::Value;

use crate::core::{Answer, Core, Request};
use crate::fire::Invocation;

/// The region whose files are kept in the service's directory itself.
const HOME_REGION: &str = "us-east-1";

/// What the invocations made off the actor share.
#[derive(Debug)]
pub(crate) struct Shared {
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) tasks: Tasks,
    pub(crate) sessions: role::Sessions,
}

/// The EventBridge Scheduler service. Nothing runs until the first request, or [`Scheduler::resume`].
#[derive(Debug)]
pub struct Scheduler {
    actors: Keyed<Core>,
    timers: Arc<Deadlines<Core>>,
    shared: Arc<Shared>,
    dir: PathBuf,
    patterns: Patterns,
}

impl Scheduler {
    /// Registers Scheduler. `ctx` says where its files live and how it reads the time.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        let peers = ctx
            .ext
            .as_ref()
            .and_then(|e| Arc::clone(e).downcast::<Peers>().ok());
        let shared = Arc::new(Shared {
            peers,
            clock: Arc::clone(&ctx.clock),
            tasks: ctx.tasks.clone(),
            sessions: role::Sessions::default(),
        });
        let on_tick = Arc::clone(&shared);
        let timers = Deadlines::new(
            Arc::clone(&ctx),
            || Request::Tick,
            Box::new(move |answer: Answer| {
                let sh = Arc::clone(&on_tick);
                Box::pin(async move { settle(&sh, answer).await })
            }),
        );
        Self {
            dir: ctx.dir.clone(),
            actors: Keyed::new("scheduler", ctx, HOME_REGION),
            timers,
            shared,
            patterns: Patterns::new(&TABLES),
        }
    }

    /// Arms the regions that have Scheduler state on disk, and fires the one-time schedules that came due while the process was down.
    /// Run once when the process starts.
    pub async fn resume(&self) {
        let mut regions = Vec::new();
        if self.dir.join(core::FILE).exists() {
            regions.push(HOME_REGION.to_owned());
        }
        if let Ok(entries) = std::fs::read_dir(self.dir.join("regions")) {
            for e in entries.flatten() {
                if e.path().join(core::FILE).exists()
                    && let Some(name) = e.file_name().to_str()
                {
                    regions.push(name.to_owned());
                }
            }
        }
        for region in regions {
            // WHY: a region that cannot be opened says so to its first caller.
            drop(self.ask(&region, Request::Resume).await);
        }
    }

    /// Asks the region's actor; waits until what it changed is on disk, starts its invocations, and re-arms its timer.
    async fn ask(
        &self,
        region: &str,
        req: Request,
    ) -> Result<(Result<Value, ApiError>, Vec<Invocation>), ApiError> {
        let actor: Arc<Lazy<Core>> = self
            .actors
            .actor(region)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let mut answer = actor
            .call(req)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        if let Some(saved) = answer.durable.take()
            && let Err(e) = saved.await
        {
            answer.reply = Err(ApiError::internal(format!("could not save: {e}")));
        }
        self.timers.arm(region, &actor, answer.next_due);
        Ok((answer.reply, std::mem::take(&mut answer.fired)))
    }

    async fn call_inner(&self, call: Call) -> Result<Value, ApiError> {
        let region = call.caller.region.clone();
        let (reply, fired) = self.ask(&region, Request::Call(Box::new(call))).await?;
        spawn_all(&self.shared, fired);
        reply
    }

    /// Whether the region's timer is running (for tests).
    #[cfg(test)]
    pub(crate) fn timer_armed(&self, region: &str) -> bool {
        self.timers.armed(region)
    }
}

/// Starts each invocation in a task of its own.
fn spawn_all(sh: &Arc<Shared>, fired: Vec<Invocation>) {
    for inv in fired {
        let sh2 = Arc::clone(sh);
        sh.tasks.spawn(async move {
            let first = deliver::attempt(&sh2, &inv, 1).await;
            retry::carry_on(sh2, inv, 1, first).await;
        });
    }
}

/// The timer's answer: waits for the save, starts the invocations, and says when it is next due.
async fn settle(sh: &Arc<Shared>, mut answer: Answer) -> Option<i64> {
    if let Some(saved) = answer.durable.take() {
        // WHY: a failed save is the writer's to report; the next call finds the same state and says so.
        drop(saved.await);
    }
    spawn_all(sh, std::mem::take(&mut answer.fired));
    answer.next_due
}

/// The region a service directory is for: `regions/<region>` below the service's own, or the home region.
pub(crate) fn region_of(dir: &Path) -> String {
    let in_regions = dir
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|p| p == "regions");
    match dir.file_name().and_then(|n| n.to_str()) {
        Some(name) if in_regions => name.to_owned(),
        _ => HOME_REGION.to_owned(),
    }
}

impl Service for Scheduler {
    fn meta(&self) -> &'static ServiceMeta {
        &META
    }

    fn tables(&self) -> &'static Tables {
        &TABLES
    }

    fn serves(&self, op: u32) -> bool {
        Operation::from_index(op).is_some_and(core::built)
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(async move {
            let op = call.op;
            let value = self.call_inner(call).await?;
            reply_json(&TABLES, &self.patterns, op, &value)
        })
    }

    fn rest(&self) -> Option<&dyn RestService> {
        Some(self)
    }

    fn web(&self) -> Option<&dyn Web> {
        Some(self)
    }
}

impl RestService for Scheduler {
    fn call_rest(
        &self,
        call: Call,
        _upload: Option<Box<dyn Upload>>,
    ) -> BoxFuture<'_, Result<RestReply, ApiError>> {
        Box::pin(async move {
            let json = self.call_inner(call).await?;
            Ok(RestReply {
                json,
                status: None,
                body: None,
            })
        })
    }
}

impl Web for Scheduler {
    fn owns(&self, _host: &str, path: &str) -> bool {
        path.starts_with(hook::PATH)
    }

    fn serve(&self, request: WebRequest) -> BoxFuture<'_, WebReply> {
        Box::pin(hook::serve(self, request))
    }
}
