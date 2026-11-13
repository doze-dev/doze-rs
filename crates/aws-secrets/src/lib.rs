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
use doze_runtime::{Ctx, Keyed};

use crate::core::{Answer, Core};

/// The region whose files are kept in the service's directory itself (state written before regions were kept apart is its).
const HOME_REGION: &str = "us-east-1";

/// The Secrets Manager service. Nothing runs until the first request.
#[derive(Debug)]
pub struct Secrets {
    /// One actor per region: a queue, parameter, secret or key belongs to the region that made it.
    actors: Keyed<Core>,
    /// Where rotations are run (the task set) and what they call (the peer bus).
    ctx: Arc<Ctx>,
}

impl Secrets {
    /// Registers Secrets Manager. `ctx` says where its files live and how it reads the time.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        Self {
            actors: Keyed::new("secretsmanager", Arc::clone(&ctx), HOME_REGION),
            ctx,
        }
    }

    /// Sends the call to the actor of the region it was signed for.
    async fn call_actor(&self, call: Call) -> Result<Answer, String> {
        let actor = self
            .actors
            .actor(&call.caller.region)
            .map_err(|e| e.to_string())?;
        actor.call(call).await.map_err(|e| e.to_string())
    }

    /// Asks the actor, and holds the answer until what it changed is on disk.
    async fn ask(&self, call: Call) -> Reply {
        let mut answer = match self.call_actor(call).await {
            Ok(answer) => answer,
            Err(e) => Answer {
                reply: Err(ApiError::internal(e.to_string())),
                durable: None,
                run: None,
            },
        };
        if let Some(saved) = answer.durable.take()
            && let Err(e) = saved.await
        {
            answer.reply = Err(ApiError::internal(format!("could not save: {e}")));
            answer.run = None;
        }
        // A rotation runs after its answer is saved, outside the actor: the function calls this service back.
        if let Some(run) = answer.run.take()
            && let Some(peers) = self
                .ctx
                .ext
                .as_ref()
                .and_then(|e| Arc::clone(e).downcast::<aws_peers::Peers>().ok())
        {
            self.ctx.tasks.spawn(rotate::run(peers, run));
        }
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

/// Whether `text` is a resource policy this service accepts. Exists so the check can be fuzzed.
#[doc(hidden)]
#[must_use]
pub fn check_resource_policy(text: &str) -> bool {
    policy::is_policy(text)
}

#[cfg(test)]
mod tests;
