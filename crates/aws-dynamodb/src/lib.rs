//! DynamoDB: tables, items, global and local secondary indexes, queries and scans with pagination, batches, transactions, PartiQL
//! (single statements, batches and transactions, translated onto the classic operations), time to live (deleted when due, by a
//! timer that runs only while an item has one), streams (read by the `aws-dynamodb-streams` service and by Lambda), point-in-time
//! recovery settings, Contributor Insights settings (stored, nothing computed) and tags, as a state machine behind a lazy actor, one
//! per region. Items live in a redb file and are read when a request needs them; nothing is kept in memory but the table records. The
//! expression languages are in `aws-expr`.
//!
//! Global tables, backups and restore, exports and imports, Kinesis streaming destinations, resource policies and vector search are
//! refused by name before their request is read, and so are the legacy parameters (`Expected`, `AttributeUpdates`, `AttributesToGet`,
//! `KeyConditions`, `QueryFilter`, `ScanFilter`, `ConditionalOperator`): none is ignored.

mod answer;
mod authz;
mod batch;
mod condition;
mod core;
mod describe;
mod disk;
mod insights;
mod invalid;
mod items;
mod keys;
mod lifecycle;
mod meta;
mod page;
mod partiql;
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
use doze_runtime::{Ctx, Deadlines, Keyed};

use crate::core::{Answer, Core, Request};

pub use crate::partiql::check as check_partiql;
pub use crate::stream::{Batch, Event, Record, StreamInfo};

/// The region whose files are kept in the service's directory itself.
const HOME_REGION: &str = "us-east-1";

/// The DynamoDB service. Nothing runs until the first request.
#[derive(Debug)]
pub struct Dynamodb {
    /// One actor per region: a table belongs to the region that made it.
    actors: Keyed<Core>,
    /// Woken when a call appended to a stream, so a consumer waiting for records looks again.
    arrived: Arc<tokio::sync::Notify>,
    /// One timer per region, asleep until the first item with a time to live expires, and none while no such item exists.
    expiry: Arc<Deadlines<Core>>,
    /// The service's directory, which `resume` looks through.
    dir: std::path::PathBuf,
    clock: Arc<dyn doze_runtime::Clock>,
}

impl Dynamodb {
    /// Registers DynamoDB. `ctx` says where its files live and how it reads the time.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        let arrived = Arc::new(tokio::sync::Notify::new());
        let wake = Arc::clone(&arrived);
        let expiry = Deadlines::new(
            Arc::clone(&ctx),
            || Request::Tick,
            Box::new(move |answer: Answer| {
                // A deletion by time to live is a REMOVE record: a consumer waiting on the stream looks again.
                if answer.appended {
                    wake.notify_waiters();
                }
                Box::pin(async move { answer.next_due })
            }),
        );
        Self {
            dir: ctx.dir.clone(),
            clock: Arc::clone(&ctx.clock),
            actors: Keyed::new("dynamodb", ctx, HOME_REGION),
            arrived,
            expiry,
        }
    }

    /// Starts the time-to-live timers of the regions that have DynamoDB state on disk. Run once when the process starts: nothing else
    /// would wake a region nobody calls after a restart, and its expired items would wait for a request. A region with nothing due is
    /// opened and left to be reaped when it has been quiet.
    pub async fn resume(&self) {
        let mut regions = Vec::new();
        if self.dir.join(disk::FILE).exists() {
            regions.push(HOME_REGION.to_owned());
        }
        if let Ok(entries) = std::fs::read_dir(self.dir.join("regions")) {
            for e in entries.flatten() {
                if e.path().join(disk::FILE).exists()
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
            if let Ok(answer) = actor.call(Request::Tick).await {
                if answer.appended {
                    self.arrived.notify_waiters();
                }
                self.expiry.arm(&region, &actor, answer.next_due);
            }
        }
    }

    /// The time by the service's clock, in milliseconds since the epoch (a stream's shard iterators expire by it).
    #[must_use]
    pub fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    /// Whether the region's time-to-live timer is running (for tests).
    #[cfg(test)]
    pub(crate) fn expiry_armed(&self, region: &str) -> bool {
        self.expiry.armed(region)
    }

    /// The streams of the region's tables (or of `table` alone), as the Streams API lists and describes them. A table's stream is
    /// listed while it is enabled.
    ///
    /// # Errors
    /// Why they cannot be read: the service could not open, or the disk failed.
    pub async fn streams(
        &self,
        region: &str,
        account: &str,
        table: Option<&str>,
    ) -> Result<Vec<StreamInfo>, String> {
        let actor = self.actors.actor(region).map_err(|e| e.to_string())?;
        let answer = actor
            .call(Request::Streams {
                account: account.to_owned(),
                region: region.to_owned(),
                table: table.map(str::to_owned),
            })
            .await
            .map_err(|e| e.to_string())?;
        answer
            .streams
            .unwrap_or_else(|| Err("no answer".to_owned()))
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
        let region = call.caller.region.clone();
        let actor = match self.actors.actor(&region) {
            Ok(actor) => actor,
            Err(e) => return Err(ApiError::internal(e.to_string())),
        };
        let Answer {
            reply,
            appended,
            next_due,
            ..
        } = match actor.call(Request::Call(Box::new(call))).await {
            Ok(a) => a,
            Err(e) => return Err(ApiError::internal(e.to_string())),
        };
        if appended {
            self.arrived.notify_waiters();
        }
        self.expiry.arm(&region, &actor, next_due);
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
