//! EventBridge: event buses, rules with event patterns or schedules, targets (with `Input`, `InputPath` and `InputTransformer`, a
//! retry policy and a dead-letter queue), archives and replay, connections and API destinations, tags and bus permissions, as a state
//! machine behind a lazy actor per region. State is on disk and survives an idle reap and a restart.
//!
//! **Events** are delivered in the call that puts them, through the peer bus, to SQS queues, SNS topics, Kinesis streams, CloudWatch
//! Logs groups and event buses of the region; to Step Functions and Lambda once those services run. A target on any other service is
//! stored and returned as sent, and **not delivered**: the log says so. An API destination is called with one HTTP request when its
//! connection is Basic or API key and its endpoint is `http://`; OAuth connections and `https://` endpoints are stored and not called.
//! Other services put events on the default bus through [`publish`].
//!
//! **Schedules are deadline-driven, not ticked.** After every call the service works out the earliest time a scheduled rule or a retry is
//! due; one timer sleeps to it and ends when nothing is pending.
//!
//! Refused by name: partner event sources, global endpoints (STAY CUT in SCOPE.md) and `CancelReplay` (a replay is finished when it is
//! started). EventBridge Scheduler is a different service (restJson) and is not in this crate.

mod archives;
mod authz;
mod buses;
mod connections;
mod core;
mod deliver;
mod deliver_http;
mod destinations;
mod event;
mod events;
mod http;
mod input;
mod jsonpath;
mod model;
mod names;
mod page;
mod pattern;
mod perms;
mod policy;
mod replays;
mod req;
mod retry;
mod route;
mod rules;
mod schedule;
mod tags;
mod targets;
mod timers;
mod util;

pub mod publish;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use aws_model_eventbridge::Operation;
use aws_model_eventbridge::meta::META;
use aws_model_eventbridge::tables::TABLES;
use aws_schema::meta::ServiceMeta;
use aws_schema::tables::Tables;
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Call, Caller, Reply, Service};
use doze_runtime::{Ctx, Keyed};

use crate::core::{Core, Request};
use crate::timers::Timers;

/// The region whose files are kept in the service's directory itself.
const HOME_REGION: &str = "us-east-1";

/// The EventBridge service. Nothing runs until the first request.
#[derive(Debug)]
pub struct Eventbridge {
    /// One actor per region: a bus belongs to the region that made it.
    actors: Keyed<Core>,
    timers: Arc<Timers>,
    /// The service's directory, which `resume` looks through.
    dir: std::path::PathBuf,
}

impl Eventbridge {
    /// Registers EventBridge. `ctx` says where its files live and how it reads the time.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        Self {
            dir: ctx.dir.clone(),
            timers: Timers::new(Arc::clone(&ctx)),
            actors: Keyed::new("eventbridge", ctx, HOME_REGION),
        }
    }

    /// Starts the timers of the regions that have EventBridge state on disk. Run once when the process starts: nothing else would wake a
    /// region that nobody calls after a restart, and its schedules would wait for a request. A region with nothing scheduled is opened
    /// and left to be reaped when it has been quiet.
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
            let Ok(actor) = self.actors.actor(&region) else {
                continue;
            };
            let caller = Caller {
                access_key_id: "doze".to_owned(),
                account_id: String::new(),
                region: region.clone(),
                endpoint: String::new(),
                service: None,
                session_token: None,
            };
            // A tick with a deadline of "never" fires nothing and answers when the first thing is due.
            if let Ok(mut answer) = actor.call(Request::Tick(caller.clone(), i64::MAX)).await {
                if let Some(saved) = answer.durable.take() {
                    // WHY: a failed save is the writer's to report; the next call finds the same state and says so.
                    drop(saved.await);
                }
                self.timers.arm(&region, &caller, &actor, answer.next_due);
            }
        }
    }

    /// Sends the call to the actor of the region it was signed for, holds the answer until what it changed is on disk, and tells the
    /// region's timer when something is next due.
    async fn ask(&self, call: Call) -> Reply {
        let region = call.caller.region.clone();
        let caller = call.caller.clone();
        let actor = match self.actors.actor(&region) {
            Ok(actor) => actor,
            Err(e) => return Err(ApiError::internal(e.to_string())),
        };
        let mut answer = match actor.call(Request::Call(call)).await {
            Ok(answer) => answer,
            Err(e) => return Err(ApiError::internal(e.to_string())),
        };
        if let Some(saved) = answer.durable.take()
            && let Err(e) = saved.await
        {
            return Err(ApiError::internal(format!("could not save: {e}")));
        }
        self.timers.arm(&region, &caller, &actor, answer.next_due);
        answer.reply
    }

    /// Whether the region has a running timer (for tests).
    #[cfg(test)]
    pub(crate) fn timer_armed(&self, region: &str) -> bool {
        self.timers.armed(region)
    }
}

impl Service for Eventbridge {
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
        Box::pin(self.ask(call))
    }
}

/// Whether `text` is an event pattern this service accepts. Exists so the parser can be fuzzed.
#[doc(hidden)]
#[must_use]
pub fn check_event_pattern(text: &str) -> bool {
    pattern::check(text)
}
