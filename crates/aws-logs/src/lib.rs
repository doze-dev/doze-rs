//! CloudWatch Logs: log groups, streams and events as a state machine behind a lazy actor. State is on disk and survives an idle
//! reap and a restart: groups, streams and the stored documents are held in memory (a few hundred records), and the events are
//! only ever read from the database, so a stream's size is bounded by its retention and not by memory.
//!
//! Built: groups, streams, `PutLogEvents` (the sequence token is ignored, as AWS now does), `GetLogEvents`, `FilterLogEvents` with
//! term, JSON and space-delimited patterns, retention (applied lazily, when a group is written or read), tags, the KMS key,
//! deletion protection, metric filters (a matching event becomes a CloudWatch data point when CloudWatch is running),
//! subscription filters (stored; nothing is delivered), and stored documents (resource, account and data protection policies,
//! query definitions). Refused by name: Insights queries, Live Tail, export and import tasks, deliveries, destinations, anomaly
//! detectors, transformers, integrations, scheduled queries and lookup tables.

mod authz;
mod core;
mod docs;
mod events;
mod filters;
mod forward;
mod groups;
mod model;
mod pattern;
mod reading;
mod req;
mod streams;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use aws_model_cloudwatch_logs::Operation;
use aws_model_cloudwatch_logs::meta::META;
use aws_model_cloudwatch_logs::tables::TABLES;
use aws_schema::meta::ServiceMeta;
use aws_schema::tables::Tables;
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Call, Reply, Service};
use doze_runtime::{Ctx, Keyed};

use crate::core::Core;

/// The region whose files are kept in the service's directory itself.
const HOME_REGION: &str = "us-east-1";

/// The CloudWatch Logs service. Nothing runs until the first request.
#[derive(Debug)]
pub struct Logs {
    /// One actor per region: a log group belongs to the region that made it.
    actors: Keyed<Core>,
}

impl Logs {
    /// Registers CloudWatch Logs. `ctx` says where its files live and how it reads the time.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        Self {
            actors: Keyed::new("logs", ctx, HOME_REGION),
        }
    }

    /// Sends the call to the actor of the region it was signed for, and holds the answer until what it changed is on disk.
    async fn ask(&self, call: Call) -> Reply {
        let actor = match self.actors.actor(&call.caller.region) {
            Ok(actor) => actor,
            Err(e) => return Err(ApiError::internal(e.to_string())),
        };
        let mut answer = match actor.call(call).await {
            Ok(answer) => answer,
            Err(e) => return Err(ApiError::internal(e.to_string())),
        };
        if let Some(saved) = answer.durable.take()
            && let Err(e) = saved.await
        {
            return Err(ApiError::internal(format!("could not save: {e}")));
        }
        answer.reply
    }
}

/// Whether `message` is selected by `pattern`; `false` for a pattern that does not parse. Exists so the pattern parser can be
/// fuzzed.
#[doc(hidden)]
#[must_use]
pub fn check_filter_pattern(pattern: &str, message: &str) -> bool {
    pattern::parse(pattern).is_ok_and(|p| p.matches(message))
}

impl Service for Logs {
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
