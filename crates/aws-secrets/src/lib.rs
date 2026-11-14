//! Secrets Manager: secrets with versions and staging labels, tags, deletion with a recovery window, resource policies and random
//! passwords, as a state machine behind a lazy actor. State is on disk, values are sealed, and it survives an idle reap and a
//! restart. Rotation drives the secret's Lambda function through its four steps after `RotateSecret` answers (`rotate.rs`);
//! replication to other regions is refused by name.

mod authz;
mod core;
mod create;
mod delete;
mod describe;
mod find;
mod kms;
mod model;
mod password;
mod policy;
mod read;
mod rotate;
mod schedule;
mod stage;
mod tags;
mod token;

use std::sync::Arc;

use aws_model_secrets_manager::Operation;
use aws_model_secrets_manager::meta::META;
use aws_model_secrets_manager::tables::TABLES;
use aws_schema::json::Violation;
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Call, Reply, Service};
use doze_runtime::{Ctx, Deadlines, Keyed};

use crate::core::{Answer, Core, Request};

/// The region whose files are kept in the service's directory itself (state written before regions were kept apart is its).
const HOME_REGION: &str = "us-east-1";

/// The Secrets Manager service. Nothing runs until the first request.
#[derive(Debug)]
pub struct Secrets {
    /// One actor per region: a queue, parameter, secret or key belongs to the region that made it.
    actors: Keyed<Core>,
    /// Where rotations are run (the task set) and what they call (the peer bus).
    ctx: Arc<Ctx>,
    /// One timer per region, asleep until the soonest scheduled rotation window opens, and none while no rotation is scheduled.
    due: Arc<Deadlines<Core>>,
}

impl Secrets {
    /// Registers Secrets Manager. `ctx` says where its files live and how it reads the time.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        let started = Arc::clone(&ctx);
        let due = Deadlines::new(
            Arc::clone(&ctx),
            || Request::Tick,
            Box::new(move |mut answer: Answer| {
                let ctx = Arc::clone(&started);
                Box::pin(async move {
                    if let Some(saved) = answer.durable.take()
                        && saved.await.is_err()
                    {
                        return answer.next_due;
                    }
                    start(&ctx, std::mem::take(&mut answer.runs));
                    answer.next_due
                })
            }),
        );
        Self {
            actors: Keyed::new("secretsmanager", Arc::clone(&ctx), HOME_REGION),
            ctx,
            due,
        }
    }

    /// Starts the rotation timers of the regions that have Secrets Manager state on disk. Run once when the process starts: nothing
    /// else would wake a region nobody calls after a restart, and its scheduled rotations would wait for a request. A region with
    /// nothing scheduled is opened and left to be reaped when it has been quiet.
    pub async fn resume(&self) {
        let mut regions = Vec::new();
        if self.ctx.dir.join(core::FILE).exists() {
            regions.push(HOME_REGION.to_owned());
        }
        if let Ok(entries) = std::fs::read_dir(self.ctx.dir.join("regions")) {
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
            // A tick starts what came due while the process was stopped, and answers when the next window opens.
            if let Ok(mut answer) = actor.call(Request::Tick).await {
                if let Some(saved) = answer.durable.take() {
                    // WHY: a failed save is the writer's to report; the next call finds the same state and says so.
                    drop(saved.await);
                }
                start(&self.ctx, std::mem::take(&mut answer.runs));
                self.due.arm(&region, &actor, answer.next_due);
            }
        }
    }

    /// Whether the region's rotation timer is running (for tests).
    #[cfg(test)]
    pub(crate) fn timer_armed(&self, region: &str) -> bool {
        self.due.armed(region)
    }

    /// Sends the call to the actor of the region it was signed for.
    async fn call_actor(&self, call: Call) -> Result<Answer, String> {
        let region = call.caller.region.clone();
        let actor = self.actors.actor(&region).map_err(|e| e.to_string())?;
        let answer = actor
            .call(Request::Call(Box::new(call)))
            .await
            .map_err(|e| e.to_string())?;
        self.due.arm(&region, &actor, answer.next_due);
        Ok(answer)
    }

    /// Asks the actor, and holds the answer until what it changed is on disk.
    async fn ask(&self, call: Call) -> Reply {
        let mut answer = match self.call_actor(call).await {
            Ok(answer) => answer,
            Err(e) => Answer {
                reply: Err(ApiError::internal(e.to_string())),
                durable: None,
                runs: Vec::new(),
                next_due: None,
            },
        };
        if let Some(saved) = answer.durable.take()
            && let Err(e) = saved.await
        {
            answer.reply = Err(ApiError::internal(format!("could not save: {e}")));
            answer.runs.clear();
        }
        start(&self.ctx, std::mem::take(&mut answer.runs));
        answer.reply
    }
}

impl Service for Secrets {
    fn meta(&self) -> &'static aws_schema::meta::ServiceMeta {
        &META
    }

    fn tables(&self) -> &'static aws_schema::tables::Tables {
        &TABLES
    }

    fn serves(&self, op: u32) -> bool {
        Operation::from_index(op).is_some_and(core::built)
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(self.ask(call))
    }

    /// Secrets Manager words a name that breaks the rule as AWS did when the scenario was recorded; the rest are the Coral
    /// front end's.
    fn invalid(&self, violation: &Violation) -> ApiError {
        let member = |ix: u32| TABLES.member(ix).map_or("", |m| m.name);
        match violation {
            Violation::Pattern { member: m, .. } | Violation::Length { member: m, .. }
                if member(*m) == "Name" =>
            {
                ApiError::modeled(
                    "ValidationException",
                    "Invalid name. Must be a valid name containing alphanumeric characters, or any of the following: -/_+=.@!",
                )
            }
            other => aws_wire::service::default_invalid(&TABLES, other),
        }
    }
}

/// Starts rotations once their answer is saved, outside the actor: the function calls this service back.
fn start(ctx: &Ctx, runs: Vec<rotate::Run>) {
    let Some(peers) = ctx
        .ext
        .as_ref()
        .and_then(|e| Arc::clone(e).downcast::<aws_peers::Peers>().ok())
    else {
        return;
    };
    for run in runs {
        ctx.tasks.spawn(rotate::run(Arc::clone(&peers), run));
    }
}

/// Whether `text` is a resource policy this service accepts. Exists so the check can be fuzzed.
#[doc(hidden)]
#[must_use]
pub fn check_resource_policy(text: &str) -> bool {
    policy::is_policy(text)
}

#[cfg(test)]
mod tests;
