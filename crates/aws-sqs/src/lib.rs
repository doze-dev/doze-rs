//! SQS: standard queues, as a state machine behind a lazy actor.
//!
//! What is built: create, delete, list and look up queues; read and set their attributes; send, receive, delete and
//! re-time messages; purge. What is not yet: FIFO queues and groups, batches, message attributes, long polling, tags,
//! permissions, dead-letter redrive and move tasks, persistence. Every operation the model has is named in
//! [`core`]'s dispatch, and an unbuilt one answers `501 NotImplemented` by name.

mod attrs;
mod core;
mod messages;
mod model;
mod queues;
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

use crate::core::Core;

/// The SQS service. Nothing runs until the first request.
#[derive(Debug)]
pub struct Sqs {
    actor: Lazy<Core>,
}

impl Sqs {
    /// Registers SQS. `ctx` says where its files would live, when it has any, and how it reads the time.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        Self {
            actor: Lazy::new("sqs", ctx),
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
            match self.actor.call(call).await {
                Ok(reply) => reply,
                Err(e) => Err(ApiError::internal(e.to_string())),
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
