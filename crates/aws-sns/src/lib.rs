//! SNS: topics (standard and FIFO), subscriptions with their confirmation rules, Publish and PublishBatch, filter policies on message
//! attributes or the message body, and fan-out to SQS through the peer bus, as a state machine behind a lazy actor per region. State is
//! on disk, and survives an idle reap and a restart.
//!
//! **What is stored and not delivered:** SQS and Lambda are delivered to. Subscriptions for `http`, `https`, `email`, `email-json` and
//! `firehose` are kept (the first four wait for a confirmation token no one is sent), and SMS and mobile-push messages are accepted
//! (decision D1) and sent nowhere; the log says so for each. The SMS sandbox and origination numbers are refused by name.

mod arn;
mod authz;
mod core;
mod deliver;
mod envelope;
mod filter;
mod filter_match;
mod message;
mod model;
mod perms;
mod platform;
mod publish;
mod req;
mod sms;
mod sub_attrs;
mod subs;
mod tags;
mod topic_attrs;
mod topics;

use std::sync::Arc;

use aws_model_sns::Operation;
use aws_model_sns::meta::META;
use aws_model_sns::tables::TABLES;
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Call, Reply, Service};
use doze_runtime::{Ctx, Keyed};

use crate::core::{Answer, Core};

/// The region whose files are kept in the service's directory itself.
const HOME_REGION: &str = "us-east-1";

/// The SNS service. Nothing runs until the first request.
#[derive(Debug)]
pub struct Sns {
    /// One actor per region: a topic belongs to the region that made it.
    actors: Keyed<Core>,
}

impl Sns {
    /// Registers SNS. `ctx` says where its files live and how it reads the time.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        Self {
            actors: Keyed::new("sns", ctx, HOME_REGION),
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
                reply: Err(ApiError::internal(e)),
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

impl Service for Sns {
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

/// Whether `text` is a filter policy this service accepts, for the scope of message attributes (`body` false) or of the message body.
/// Exists so the parser can be fuzzed.
#[doc(hidden)]
#[must_use]
pub fn check_filter_policy(text: &str, body: bool) -> bool {
    filter::parse(text, body).is_ok()
}

#[cfg(test)]
mod tests;
