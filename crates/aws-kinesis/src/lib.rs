//! Kinesis Data Streams: streams, shards, records, iterators, consumers and tags, as a state machine behind a lazy actor. State is on
//! disk and survives an idle reap and a restart: stream metadata is kept in memory, and the records are only ever read from the
//! database, so a stream's size is bounded by its retention and not by memory.
//!
//! Resharding (`SplitShard`, `MergeShards`, `UpdateShardCount`) closes shards and opens children that name them; a closed shard is
//! read to its end and then answers with no `NextShardIterator` and its `ChildShards`.
//!
//! Enhanced fan-out: a registered consumer subscribes to a shard (`SubscribeToShard`) and is pushed its records as an event stream for
//! five minutes (`fanout`, `subscribe`).
//!
//! Refused by name: channels, and `UpdateStreamRecordDistributionStrategy`.

mod authz;
mod billing;
mod consumers;
mod core;
mod fanout;
mod model;
mod reading;
mod records;
mod req;
mod reshard;
mod settings;
mod storage;
mod streams;
mod subscribe;
mod tags;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use crate::core::Core;
use aws_model_kinesis::Operation;
use aws_model_kinesis::meta::META;
use aws_model_kinesis::tables::TABLES;
use aws_schema::json::Violation;
use aws_schema::meta::ServiceMeta;
use aws_schema::tables::Tables;
use aws_wire::ApiError;
use aws_wire::rest::Download;
use aws_wire::service::{BoxFuture, Call, Reply, Service};
pub use doze_runtime::Signal;
use doze_runtime::{Ctx, Keyed};

/// The order a sequence number sorts at, as a number: what a reader that orders records (a Lambda event source mapping) compares.
/// `None` for text that is not a sequence number.
#[must_use]
pub fn sequence_value(text: &str) -> Option<u128> {
    model::seq_parse(text)
}

/// The key a stream is watched by: streams are kept per region, by name.
fn key(region: &str, name: &str) -> String {
    format!("{region}/{name}")
}

/// The region whose files are kept in the service's directory itself.
const HOME_REGION: &str = "us-east-1";

/// The Kinesis service. Nothing runs until the first request.
#[derive(Debug)]
pub struct Kinesis {
    /// One actor per region: a stream belongs to the region that made it.
    actors: Keyed<Core>,
    /// The streams consumers wait on, by `region/name`, each woken when records are appended to it (and are on disk)
    /// ([`Kinesis::watch_stream`]).
    signals: Arc<doze_runtime::Signals>,
    /// The open `SubscribeToShard` subscriptions.
    subs: Arc<subscribe::Subscriptions>,
    ctx: Arc<Ctx>,
}

impl Kinesis {
    /// Registers Kinesis. `ctx` says where its files live and how it reads the time.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        Self {
            actors: Keyed::new("kinesis", Arc::clone(&ctx), HOME_REGION),
            signals: Arc::default(),
            subs: Arc::default(),
            ctx,
        }
    }

    /// Opens a subscription in the call's region and streams it.
    async fn subscribe(&self, call: Call) -> Result<Download, ApiError> {
        let region = call.caller.region.clone();
        let account = call.caller.account_id.clone();
        let actor = self
            .actors
            .actor(&region)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let (reply, opened) = tokio::sync::oneshot::channel();
        let ask = fanout::FanoutAsk::Open(Box::new(call), reply);
        actor
            .call(crate::core::Ask::Fanout(Box::new(ask)))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let position = opened
            .await
            .map_err(|_| ApiError::internal("the subscription was not opened"))??;
        let (claim, takeover) = self.subs.claim(&region, &position);
        let appended = self.signals.watch(key(&region, &position.stream));
        let session = subscribe::Session {
            actor,
            account,
            position,
            appended,
            _claim: claim,
            takeover,
        };
        Ok(subscribe::stream(&self.ctx.tasks, session))
    }

    /// Watches the stream `stream_arn` names: the notification a Lambda event source mapping waits on instead of polling. Woken
    /// only when records are appended to that stream; enable [`Signal::notified`] before reading, so none is missed.
    #[must_use]
    pub fn watch_stream(&self, stream_arn: &str) -> Signal {
        let region = stream_arn.split(':').nth(3).unwrap_or_default();
        let name = stream_arn
            .rsplit_once("stream/")
            .map_or(stream_arn, |(_, n)| n);
        self.signals.watch(key(region, name))
    }

    /// Sends the call to the actor of the region it was signed for, and holds the answer until what it changed is on disk.
    async fn ask(&self, call: Call) -> Reply {
        let region = call.caller.region.clone();
        let actor = match self.actors.actor(&region) {
            Ok(actor) => actor,
            Err(e) => return Err(ApiError::internal(e.to_string())),
        };
        let mut answer = match actor.call(crate::core::Ask::Call(Box::new(call))).await {
            Ok(answer) => answer,
            Err(e) => return Err(ApiError::internal(e.to_string())),
        };
        if let Some(saved) = answer.durable.take()
            && let Err(e) = saved.await
        {
            return Err(ApiError::internal(format!("could not save: {e}")));
        }
        for (name, shards) in &answer.appended {
            self.signals
                .tell_parts(&key(&region, name), shards.iter().map(String::as_str));
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

    fn streams(&self, op: u32) -> bool {
        Operation::from_index(op) == Some(Operation::SubscribeToShard)
    }

    fn call_stream(&self, call: Call) -> BoxFuture<'_, Result<Download, ApiError>> {
        Box::pin(self.subscribe(call))
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

    fn storage(&self) -> Option<&dyn aws_wire::storage::Storage> {
        Some(self)
    }
}
