//! Kinesis Data Streams: streams, shards, records, iterators, consumers and tags, as a state machine behind a lazy actor. State is on
//! disk and survives an idle reap and a restart: stream metadata is kept in memory, and the records are only ever read from the
//! database, so a stream's size is bounded by its retention and not by memory.
//!
//! Refused by name: resharding (`SplitShard`, `MergeShards`, `UpdateShardCount`: the shard records keep their lineage so adding
//! them later changes nothing stored), `SubscribeToShard` (HTTP/2), channels, and the account-settings operations.

mod authz;
mod consumers;
mod core;
mod model;
mod reading;
mod records;
mod req;
mod settings;
mod streams;
mod tags;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use aws_model_kinesis::Operation;
use aws_model_kinesis::meta::META;
use aws_model_kinesis::tables::TABLES;
use aws_schema::json::Violation;
use aws_schema::meta::ServiceMeta;
use aws_schema::tables::Tables;
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Call, Reply, Service};
use doze_runtime::{Ctx, Keyed};
use tokio::sync::watch;

use crate::core::Core;

/// The region whose files are kept in the service's directory itself.
const HOME_REGION: &str = "us-east-1";

/// The Kinesis service. Nothing runs until the first request.
#[derive(Debug)]
pub struct Kinesis {
    /// One actor per region: a stream belongs to the region that made it.
    actors: Keyed<Core>,
    /// A counter that goes up each time records are appended (and are on disk). A poller that wants to know when a stream has
    /// something new waits on it ([`Kinesis::appended`]).
    appended: watch::Sender<u64>,
}

impl Kinesis {
    /// Registers Kinesis. `ctx` says where its files live and how it reads the time.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        Self {
            actors: Keyed::new("kinesis", ctx, HOME_REGION),
            appended: watch::channel(0).0,
        }
    }

    /// A receiver that changes whenever records have been appended to any stream: the notification a Lambda event source mapping
    /// waits on instead of polling. It says nothing of which stream; the waiter reads and finds out.
    #[must_use]
    pub fn appended(&self) -> watch::Receiver<u64> {
        self.appended.subscribe()
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
        if answer.appended {
            self.appended.send_modify(|n| *n += 1);
        }
        answer.reply
    }
}

impl Service for Kinesis {
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

    /// The Coral front end's wording, except that Kinesis prints a pattern as the model writes it, anchors and all (recorded
    /// 2026-11-09: `^(0|([1-9]\d{0,38}))$`; IAM and STS print theirs without).
    fn invalid(&self, violation: &Violation) -> ApiError {
        let mut e = aws_wire::service::default_invalid(&TABLES, violation);
        if let Violation::Pattern { member, .. } = violation
            && let Some(pattern) = TABLES
                .member(*member)
                .and_then(|m| TABLES.cons(m.cons))
                .and_then(|c| c.pattern)
            && let Some(at) = e.message.find("pattern: ")
        {
            e.message.truncate(at + "pattern: ".len());
            e.message.push_str(pattern);
        }
        e
    }
}
