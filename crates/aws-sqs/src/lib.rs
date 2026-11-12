//! SQS: all 23 operations, as a state machine behind a lazy actor. Standard and FIFO queues, batches, message attributes,
//! long polling, tags, permissions, dead-letter queues and move tasks. State is on disk ([`persist`]): it survives an idle
//! reap and a restart. Every operation the model has is named in [`core`]'s dispatch.

mod attrs;
mod batch;
mod core;
mod fifo;
mod messages;
mod model;
mod moves;
mod msgattr;
mod persist;
mod policy;
mod queues;
mod redrive;
mod send;
mod tags;
mod url;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use aws_model_sqs::meta::META;
use aws_model_sqs::tables::TABLES;
use aws_schema::json::Violation;
use aws_schema::meta::ServiceMeta;
use aws_schema::tables::Tables;
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Call, Reply, Service};
use doze_runtime::{Ctx, Lazy};

use crate::core::{Answer, Core};

/// MD5 of some bytes, as lower-case hex.
pub(crate) fn md5_hex(bytes: &[u8]) -> String {
    use md5::{Digest, Md5};
    aws_core::hex::encode(&Md5::digest(bytes))
}

/// The SQS service. Nothing runs until the first request.
#[derive(Debug)]
pub struct Sqs {
    actor: Lazy<Core>,
    /// Woken by anything that could give a waiting receive a message.
    arrived: tokio::sync::Notify,
}

impl Sqs {
    /// Registers SQS. `ctx` says where its files would live, when it has any, and how it reads the time.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        Self {
            actor: Lazy::new("sqs", ctx),
            arrived: tokio::sync::Notify::new(),
        }
    }

    /// Asks the actor, and holds the answer until what it changed is on disk.
    async fn ask(&self, call: Call) -> Answer {
        let mut answer = self.ask_actor(call).await;
        if let Some(saved) = answer.durable.take()
            && let Err(e) = saved.await
        {
            answer.reply = Err(ApiError::internal(format!("could not save: {e}")));
        }
        answer
    }

    async fn ask_actor(&self, call: Call) -> Answer {
        match self.actor.call(call).await {
            Ok(answer) => answer,
            Err(e) => Answer {
                reply: Err(ApiError::internal(e.to_string())),
                poll: None,
                durable: None,
            },
        }
    }

    /// A long poll: asks again when something has arrived, or every quarter second (a message can become visible by the clock
    /// alone), until one is found or `seconds` have passed. The actor is free the whole time. `empty` is what to answer at
    /// the end.
    async fn wait_for_messages(&self, call: Call, seconds: u32, empty: Reply) -> Reply {
        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_secs(u64::from(seconds));
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            if left.is_zero() {
                return empty;
            }
            let tick = left.min(std::time::Duration::from_millis(250));
            tokio::select! {
                () = self.arrived.notified() => {}
                () = tokio::time::sleep(tick) => {}
            }
            let again = self.ask(call.duplicate()).await;
            if again.poll.is_none() {
                return again.reply;
            }
        }
    }
}

impl Sqs {
    /// The answer for a failed model check, without needing a service value (tests).
    #[cfg(test)]
    pub(crate) fn invalid_for_tests(violation: &Violation) -> ApiError {
        invalid(violation)
    }
}

impl Service for Sqs {
    fn meta(&self) -> &'static ServiceMeta {
        &META
    }

    fn tables(&self) -> &'static Tables {
        &TABLES
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(async move {
            let receive = aws_model_sqs::Operation::from_index(call.op)
                == Some(aws_model_sqs::Operation::ReceiveMessage);
            let first = self.ask(call.duplicate()).await;
            if !receive {
                // A send, a visibility change, a purge, a redrive: something a waiting receive may now find.
                self.arrived.notify_waiters();
                return first.reply;
            }
            match first.poll {
                Some(seconds) => self.wait_for_messages(call, seconds, first.reply).await,
                None => first.reply,
            }
        })
    }

    /// SQS words a request that fails its model's checks as `MissingParameter` or `InvalidParameterValue`, not as the Coral
    /// front end's `ValidationException`.
    fn invalid(&self, violation: &Violation) -> ApiError {
        invalid(violation)
    }
}

fn invalid(violation: &Violation) -> ApiError {
    let member = |ix: u32| TABLES.member(ix).map_or("", |m| m.name);
    match violation {
        Violation::Missing { member: m, .. } => core::missing(member(*m)),
        Violation::Pattern { member: m } | Violation::Length { member: m }
            if member(*m) == "QueueName" =>
        {
            core::invalid_parameter(
                "Can only include alphanumeric characters, hyphens, or underscores. 1 to 80 in length",
            )
        }
        Violation::Syntax { .. } | Violation::TooDeep => ApiError::new(
            400,
            "MalformedInput",
            "The request body could not be parsed.",
        ),
        other => core::invalid_parameter(format!("The request is not valid: {other:?}")),
    }
}
