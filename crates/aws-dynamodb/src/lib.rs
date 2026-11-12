//! DynamoDB: tables, items, global and local secondary indexes, queries and scans with pagination, batches, transactions, time to
//! live, streams, point-in-time recovery settings and tags, as a state machine behind a lazy actor, one per region. Items live in a
//! redb file and are read when a request needs them; nothing is kept in memory but the table records. The expression languages are
//! in `aws-expr`.
//!
//! Global tables, backups and restore, exports and imports, PartiQL, contributor insights, Kinesis streaming destinations, resource
//! policies and vector search are refused by name before their request is read, and so are the legacy parameters (`Expected`,
//! `AttributeUpdates`, `AttributesToGet`, `KeyConditions`, `QueryFilter`, `ScanFilter`, `ConditionalOperator`): none is ignored.

mod answer;
mod authz;
mod batch;
mod condition;
mod core;
mod describe;
mod disk;
mod invalid;
mod items;
mod keys;
mod lifecycle;
mod meta;
mod page;
mod query;
mod req;
mod schema;
mod settings;
mod stage;
mod stream;
mod tables;
mod tables_update;
mod txn;

use std::sync::Arc;

use aws_model_dynamodb::Operation;
use aws_model_dynamodb::meta::META;
use aws_model_dynamodb::tables::TABLES;
use aws_schema::json::Violation;
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Call, Reply, Service};
use doze_runtime::{Ctx, Keyed};

use crate::core::{Answer, Core, Request};

pub use crate::stream::{Batch, Event, Record};

/// The region whose files are kept in the service's directory itself.
const HOME_REGION: &str = "us-east-1";

/// The DynamoDB service. Nothing runs until the first request.
#[derive(Debug)]
pub struct Dynamodb {
    /// One actor per region: a table belongs to the region that made it.
    actors: Keyed<Core>,
    /// Woken when a call appended to a stream, so a consumer waiting for records looks again.
    arrived: tokio::sync::Notify,
}

impl Dynamodb {
    /// Registers DynamoDB. `ctx` says where its files live and how it reads the time.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        Self {
            actors: Keyed::new("dynamodb", ctx, HOME_REGION),
            arrived: tokio::sync::Notify::new(),
        }
    }

    /// Resolves when some call has appended to a stream since it was last awaited. A consumer reads the records, and waits here when
    /// there are none.
    pub async fn stream_arrived(&self) {
        self.arrived.notified().await;
    }

    /// The records of the stream `stream_arn` after the sequence number `after` (0 for from the start), at most `limit`.
    ///
    /// # Errors
    /// Why the stream cannot be read: it is not the current stream of a table, or the service could not open.
    pub async fn stream_records(
        &self,
        region: &str,
        stream_arn: &str,
        after: u64,
        limit: usize,
    ) -> Result<Batch, String> {
        let actor = self.actors.actor(region).map_err(|e| e.to_string())?;
        let answer = actor
            .call(Request::Stream {
                stream_arn: stream_arn.to_owned(),
                after,
                limit,
            })
            .await
            .map_err(|e| e.to_string())?;
        answer.stream.unwrap_or_else(|| Err("no answer".to_owned()))
    }

    async fn ask(&self, call: Call) -> Reply {
        let answer = match self.actors.actor(&call.caller.region) {
            Ok(actor) => actor
                .call(Request::Call(Box::new(call)))
                .await
                .map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };
        let Answer {
            reply, appended, ..
        } = match answer {
            Ok(a) => a,
            Err(e) => return Err(ApiError::internal(e)),
        };
        if appended {
            self.arrived.notify_waiters();
        }
        reply
    }
}

impl Service for Dynamodb {
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

    fn invalid_in(&self, violation: &Violation, body: &str) -> ApiError {
        if matches!(violation, Violation::Length { .. })
            && let Some(e) = invalid::batch_write(body)
        {
            return e;
        }
        self.invalid(violation)
    }

    /// A union with other than one member set is DynamoDB's own refusal; the rest is the front end's.
    fn invalid(&self, violation: &Violation) -> ApiError {
        match violation {
            Violation::Union { .. } => req::validation(
                "Supplied AttributeValue has more than one datatypes set, must contain exactly one of the supported datatypes",
            ),
            other => aws_wire::service::default_invalid(&TABLES, other),
        }
    }
}

#[cfg(test)]
mod tests;
