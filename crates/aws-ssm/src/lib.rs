//! SSM Parameter Store: parameters with versions, labels, tags and paths, as a state machine behind a lazy actor. State is
//! on disk, and survives an idle reap and a restart. The other half of SSM (Run Command, Automation, Session Manager,
//! associations, inventory, patching, maintenance windows) is refused by name before its request is read.

mod authz;
mod core;
mod kms;
mod label;
mod list;
mod model;
mod name;
mod page;
mod policy;
mod read;
mod reference;
mod tags;
mod write;

use std::sync::Arc;

use aws_model_ssm::Operation;
use aws_model_ssm::meta::META;
use aws_model_ssm::tables::TABLES;
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Call, Reply, Service};
use doze_runtime::{Ctx, Keyed};

use crate::core::{Answer, Core};

/// The region whose files are kept in the service's directory itself (state written before regions were kept apart is its).
const HOME_REGION: &str = "us-east-1";

/// The Parameter Store service. Nothing runs until the first request.
#[derive(Debug)]
pub struct Ssm {
    /// One actor per region: a queue, parameter, secret or key belongs to the region that made it.
    actors: Keyed<Core>,
}

impl Ssm {
    /// Registers Parameter Store. `ctx` says where its files live and how it reads the time.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        Self {
            actors: Keyed::new("ssm", ctx, HOME_REGION),
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
            },
        };
        if let Some(saved) = answer.durable.take()
            && let Err(e) = saved.await
        {
            answer.reply = Err(ApiError::internal(format!("could not save: {e}")));
        }
        answer.reply
    }
}

impl Service for Ssm {
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
}

#[cfg(test)]
mod tests;

/// Whether `text` is a policies document this service accepts. Exists so the parser can be fuzzed.
#[doc(hidden)]
#[must_use]
pub fn check_policies(text: &str) -> bool {
    policy::parse(text).is_ok()
}
