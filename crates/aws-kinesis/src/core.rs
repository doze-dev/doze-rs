//! The state a Kinesis service owns, and the dispatch from an operation to the code that does it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use aws_model_kinesis::Operation;
use aws_model_kinesis::tables::TABLES;
use aws_peers::{Mode, Peers};
use aws_schema::patterns::Patterns;
use aws_wire::ApiError;
use aws_wire::handler::reply_json;
use aws_wire::service::{Call, Reply};
use doze_runtime::{Clock, Ctx};
use doze_store::records::Records;
use doze_store::rows::RowTable;
use doze_store::{Ack, Db, StoreError};
use serde_json::Value;

use crate::model::Stream;
use crate::req::Req;

const FILE: &str = "kinesis.redb";
const STREAMS: &str = "streams";
const RECORDS: &str = "records";
/// The version of every record here. A change of a record's shape is a new version with a branch that reads the old one.
const VERSION: u8 = 1;

/// All of Kinesis in one region: its streams, and the records of every stream (on disk, never loaded).
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) streams: Records<Stream>,
    pub(crate) rows: RowTable,
    pub(crate) db: Option<Db>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) patterns: Patterns,
    /// When each stream's expired records were last removed (not persisted: a restart sweeps once).
    pub(crate) trimmed: HashMap<String, i64>,
    /// Set by a call that appended records, so the service can wake whoever waits for them.
    pub(crate) appended: bool,
}

/// What the actor answers: the reply, the commit it must wait for, and whether records were appended.
#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) reply: Reply,
    pub(crate) durable: Option<Ack>,
    pub(crate) appended: bool,
}

/// The operations this service runs. Every other is refused by name before its request is read.
pub(crate) fn built(op: Operation) -> bool {
    use Operation as O;
    matches!(
        op,
        O::CreateStream
            | O::DeleteStream
            | O::DescribeStream
            | O::DescribeStreamSummary
            | O::ListStreams
            | O::ListShards
            | O::PutRecord
            | O::PutRecords
            | O::GetShardIterator
            | O::GetRecords
            | O::IncreaseStreamRetentionPeriod
            | O::DecreaseStreamRetentionPeriod
            | O::AddTagsToStream
            | O::RemoveTagsFromStream
            | O::ListTagsForStream
            | O::TagResource
            | O::UntagResource
            | O::ListTagsForResource
            | O::StartStreamEncryption
            | O::StopStreamEncryption
            | O::EnableEnhancedMonitoring
            | O::DisableEnhancedMonitoring
            | O::UpdateStreamMode
            | O::UpdateMaxRecordSize
            | O::UpdateStreamWarmThroughput
            | O::DescribeLimits
            | O::PutResourcePolicy
            | O::GetResourcePolicy
            | O::DeleteResourcePolicy
            | O::RegisterStreamConsumer
            | O::DeregisterStreamConsumer
            | O::DescribeStreamConsumer
            | O::ListStreamConsumers
    )
}

impl doze_runtime::Service for Core {
    type Request = Call;
    type Response = Answer;
    type Error = StoreError;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, StoreError> {
        Self::open_in(&ctx.dir, Arc::clone(&ctx.clock), peers_of(ctx))
    }

    async fn handle(&mut self, call: Call) -> Answer {
        if let Some(peers) = self.peers.clone()
            && peers.mode() != Mode::Off
            && let Err(refusal) = peers
                .authorize(&call.caller, crate::authz::needs(&call))
                .await
        {
            return Answer {
                reply: Err(refusal),
                durable: None,
                appended: false,
            };
        }
        self.appended = false;
        let mut reply = self.dispatch(&call);
        let durable = match self.flush() {
            Ok(ack) => ack,
            Err(e) => {
                reply = Err(ApiError::internal(format!("could not save: {e}")));
                None
            }
        };
        Answer {
            reply,
            durable,
            appended: self.appended,
        }
    }

    async fn close(self) {
        if let Some(db) = self.db {
            db.close();
        }
    }
}

impl Core {
    /// Kinesis with no disk (the rules alone are under test).
    #[cfg(test)]
    pub(crate) fn in_memory(clock: Arc<dyn Clock>) -> Self {
        Self::with(
            clock,
            Records::new(STREAMS, "stream", VERSION),
            RowTable::new(RECORDS, true),
            None,
            None,
        )
    }

    pub(crate) fn open_in(
        dir: &Path,
        clock: Arc<dyn Clock>,
        peers: Option<Arc<Peers>>,
    ) -> Result<Self, StoreError> {
        let db = Db::open(dir, FILE, &[STREAMS, RECORDS])?;
        let read = db.read()?;
        let streams = Records::load(STREAMS, "stream", VERSION, &read)?;
        drop(read);
        Ok(Self::with(
            clock,
            streams,
            RowTable::new(RECORDS, false),
            Some(db),
            peers,
        ))
    }

    fn with(
        clock: Arc<dyn Clock>,
        streams: Records<Stream>,
        rows: RowTable,
        db: Option<Db>,
        peers: Option<Arc<Peers>>,
    ) -> Self {
        Self {
            streams,
            rows,
            db,
            clock,
            peers,
            patterns: Patterns::new(&TABLES),
            trimmed: HashMap::new(),
            appended: false,
        }
    }

    pub(crate) fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    /// Sends what the last call changed to the writer; the [`Ack`] resolves when it is on disk.
    pub(crate) fn flush(&mut self) -> Result<Option<Ack>, StoreError> {
        let changes = vec![self.streams.stage()?];
        let rows = vec![self.rows.stage()];
        Ok(self
            .db
            .as_ref()
            .and_then(|db| db.submit_with_rows(changes, rows)))
    }

    /// Runs the call and writes the answer as the operation's output.
    pub(crate) fn dispatch(&mut self, call: &Call) -> Reply {
        let Some(op) = Operation::from_index(call.op).filter(|op| built(*op)) else {
            return Err(not_built());
        };
        let r = Req::new(call);
        let value = self.run(op, &r)?;
        reply_json(&TABLES, &self.patterns, call.op, &value)
    }

    fn run(&mut self, op: Operation, r: &Req<'_>) -> Result<Value, ApiError> {
        use Operation as O;
        match op {
            O::CreateStream => self.create_stream(r),
            O::DeleteStream => self.delete_stream(r),
            O::DescribeStream => self.describe_stream(r),
            O::DescribeStreamSummary => self.describe_stream_summary(r),
            O::ListStreams => Ok(self.list_streams(r)),
            O::ListShards => self.list_shards(r),
            O::PutRecord => self.put_record(r),
            O::PutRecords => self.put_records(r),
            O::GetShardIterator => self.get_shard_iterator(r),
            O::GetRecords => self.get_records(r),
            O::IncreaseStreamRetentionPeriod => self.change_retention(r, true),
            O::DecreaseStreamRetentionPeriod => self.change_retention(r, false),
            O::AddTagsToStream | O::TagResource => self.add_tags(r),
            O::RemoveTagsFromStream | O::UntagResource => self.remove_tags(r),
            O::ListTagsForStream | O::ListTagsForResource => self.list_tags(r),
            O::StartStreamEncryption => self.start_encryption(r),
            O::StopStreamEncryption => self.stop_encryption(r),
            O::EnableEnhancedMonitoring => self.monitoring(r, true),
            O::DisableEnhancedMonitoring => self.monitoring(r, false),
            O::UpdateStreamMode => self.update_mode(r),
            O::UpdateMaxRecordSize => self.update_record_size(r),
            O::UpdateStreamWarmThroughput => self.update_warm_throughput(r),
            O::DescribeLimits => Ok(self.describe_limits()),
            O::PutResourcePolicy => self.put_policy(r),
            O::GetResourcePolicy => self.get_policy(r),
            O::DeleteResourcePolicy => self.delete_policy(r),
            O::RegisterStreamConsumer => self.register_consumer(r),
            O::DeregisterStreamConsumer => self.deregister_consumer(r),
            O::DescribeStreamConsumer => self.describe_consumer(r),
            O::ListStreamConsumers => self.list_consumers(r),
            _ => Err(not_built()),
        }
    }

    /// The stream the request names.
    pub(crate) fn stream(&self, r: &Req<'_>) -> Result<&Stream, ApiError> {
        let name = r.stream_name().unwrap_or_default();
        self.streams
            .get(&name)
            .ok_or_else(|| not_found(&name, r.account()))
    }

    pub(crate) fn stream_mut(&mut self, r: &Req<'_>) -> Result<&mut Stream, ApiError> {
        let name = r.stream_name().unwrap_or_default();
        let account = r.account().to_owned();
        self.streams
            .get_mut(&name)
            .ok_or_else(|| not_found(&name, &account))
    }
}

/// An operation that is refused by name.
pub(crate) fn not_built() -> ApiError {
    ApiError::new(
        501,
        "NotImplemented",
        "This operation is not built in doze-aws.",
    )
}

/// Recorded on AWS (2026-11-09): `Stream <name> under account <account> not found.`
pub(crate) fn not_found(name: &str, account: &str) -> ApiError {
    ApiError::modeled(
        "ResourceNotFoundException",
        format!("Stream {name} under account {account} not found."),
    )
}

/// The bus the host handed this service, if it did.
fn peers_of(ctx: &Ctx) -> Option<Arc<Peers>> {
    ctx.ext
        .as_ref()
        .and_then(|e| Arc::clone(e).downcast::<Peers>().ok())
}

/// A fresh stream id (`^[a-z0-9]{20}-[a-z0-9]{3}$`).
pub(crate) fn new_stream_id() -> String {
    let word = |n: usize| {
        aws_seal::alphanumeric(n)
            .unwrap_or_default()
            .to_ascii_lowercase()
    };
    format!("{}-{}", word(20), word(3))
}
