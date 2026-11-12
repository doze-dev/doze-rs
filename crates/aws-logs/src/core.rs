//! The state a CloudWatch Logs service owns, and the dispatch from an operation to the code that does it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use aws_model_cloudwatch_logs::Operation;
use aws_model_cloudwatch_logs::tables::TABLES;
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

use crate::model::{Doc, Group, Stream};
use crate::req::Req;

const FILE: &str = "logs.redb";
const GROUPS: &str = "groups";
const STREAMS: &str = "streams";
const DOCS: &str = "docs";
const EVENTS: &str = "events";
const HELD: &str = "held";
/// The version of every record here. A change of a record's shape is a new version with a branch that reads the old one.
pub(crate) const VERSION: u8 = 1;

/// All of CloudWatch Logs in one region. Groups, streams and documents are held in memory; events are only ever read from the
/// database, so a stream's size is bounded by its retention and not by memory.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) groups: Records<Group>,
    pub(crate) streams: Records<Stream>,
    pub(crate) docs: Records<Doc>,
    pub(crate) events: RowTable,
    pub(crate) db: Option<Db>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) patterns: Patterns,
    /// When each group's events past its retention were last removed (not persisted: a restart sweeps once).
    pub(crate) swept: HashMap<String, i64>,
    /// Metric data points the last call made through metric filters, for CloudWatch (sent once the call is answered).
    pub(crate) emit: Vec<Value>,
    /// What subscription filters deliver, once the call is answered.
    pub(crate) forward: Vec<crate::forward::Forward>,
    /// Events held per stream (by stream key), for the local cap. A stream with no entry (made before the count was kept) is counted
    /// when first needed.
    pub(crate) held: Records<u64>,
    /// The minute a metric filter last reported its default value, by group and filter (not persisted: at worst a restart reports
    /// it once more in the same minute).
    pub(crate) defaulted: HashMap<(String, String), i64>,
    /// The local bound on an event's age (`--logs-retention-days`), whatever the group's retention; `None` keeps them.
    pub(crate) keep_ms: Option<i64>,
    /// The local bound on the events one group holds (`--logs-max-events`); 0 is none.
    pub(crate) max_events: u64,
}

/// What the actor answers: the reply, and the commit it must wait for.
#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) reply: Reply,
    pub(crate) durable: Option<Ack>,
}

/// The operations this service runs. Every other is refused by name before its request is read.
pub(crate) fn built(op: Operation) -> bool {
    use Operation as O;
    matches!(
        op,
        O::CreateLogGroup
            | O::DeleteLogGroup
            | O::DescribeLogGroups
            | O::ListLogGroups
            | O::CreateLogStream
            | O::DeleteLogStream
            | O::DescribeLogStreams
            | O::PutLogEvents
            | O::GetLogEvents
            | O::FilterLogEvents
            | O::PutRetentionPolicy
            | O::DeleteRetentionPolicy
            | O::TagLogGroup
            | O::UntagLogGroup
            | O::ListTagsLogGroup
            | O::TagResource
            | O::UntagResource
            | O::ListTagsForResource
            | O::AssociateKmsKey
            | O::DisassociateKmsKey
            | O::PutLogGroupDeletionProtection
            | O::PutMetricFilter
            | O::DeleteMetricFilter
            | O::DescribeMetricFilters
            | O::TestMetricFilter
            | O::PutSubscriptionFilter
            | O::DeleteSubscriptionFilter
            | O::DescribeSubscriptionFilters
            | O::PutResourcePolicy
            | O::DescribeResourcePolicies
            | O::DeleteResourcePolicy
            | O::PutAccountPolicy
            | O::DescribeAccountPolicies
            | O::DeleteAccountPolicy
            | O::PutQueryDefinition
            | O::DescribeQueryDefinitions
            | O::DeleteQueryDefinition
            | O::PutDataProtectionPolicy
            | O::GetDataProtectionPolicy
            | O::DeleteDataProtectionPolicy
    )
}

impl doze_runtime::Service for Core {
    type Request = Call;
    type Response = Answer;
    type Error = StoreError;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, StoreError> {
        let mut core = Self::open_in(&ctx.dir, Arc::clone(&ctx.clock), peers_of(ctx))?;
        core.set(settings_of(ctx));
        Ok(core)
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
            };
        }
        let mut reply = self.dispatch(&call);
        let durable = match self.flush() {
            Ok(ack) => ack,
            Err(e) => {
                reply = Err(ApiError::internal(format!("could not save: {e}")));
                None
            }
        };
        self.send_metrics(&call).await;
        self.send_forwards(&call).await;
        Answer { reply, durable }
    }

    async fn close(self) {
        if let Some(db) = self.db {
            db.close();
        }
    }
}

impl Core {
    /// CloudWatch Logs with no disk (the rules alone are under test).
    #[cfg(test)]
    pub(crate) fn in_memory(clock: Arc<dyn Clock>, peers: Option<Arc<Peers>>) -> Self {
        Self::with(
            clock,
            (
                Records::new(GROUPS, "log group", VERSION),
                Records::new(STREAMS, "log stream", VERSION),
                Records::new(DOCS, "document", VERSION),
                Records::new(HELD, "event count", VERSION),
            ),
            RowTable::new(EVENTS, true),
            None,
            peers,
        )
    }

    pub(crate) fn open_in(
        dir: &Path,
        clock: Arc<dyn Clock>,
        peers: Option<Arc<Peers>>,
    ) -> Result<Self, StoreError> {
        let db = Db::open(dir, FILE, &[GROUPS, STREAMS, DOCS, HELD, EVENTS])?;
        let read = db.read()?;
        let loaded = (
            Records::load(GROUPS, "log group", VERSION, &read)?,
            Records::load(STREAMS, "log stream", VERSION, &read)?,
            Records::load(DOCS, "document", VERSION, &read)?,
            Records::load(HELD, "event count", VERSION, &read)?,
        );
        drop(read);
        Ok(Self::with(
            clock,
            loaded,
            RowTable::new(EVENTS, false),
            Some(db),
            peers,
        ))
    }

    fn with(
        clock: Arc<dyn Clock>,
        (groups, streams, docs, held): (
            Records<Group>,
            Records<Stream>,
            Records<Doc>,
            Records<u64>,
        ),
        events: RowTable,
        db: Option<Db>,
        peers: Option<Arc<Peers>>,
    ) -> Self {
        let settings = crate::Settings::default();
        Self {
            groups,
            streams,
            docs,
            held,
            events,
            db,
            clock,
            peers,
            patterns: Patterns::new(&TABLES),
            swept: HashMap::new(),
            emit: Vec::new(),
            forward: Vec::new(),
            defaulted: HashMap::new(),
            keep_ms: settings.retention.map(ms),
            max_events: settings.max_events,
        }
    }

    /// Takes the local settings.
    pub(crate) fn set(&mut self, settings: crate::Settings) {
        self.keep_ms = settings.retention.map(ms);
        self.max_events = settings.max_events;
    }

    pub(crate) fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    /// Sends what the last call changed to the writer; the [`Ack`] resolves when it is on disk.
    pub(crate) fn flush(&mut self) -> Result<Option<Ack>, StoreError> {
        let changes = vec![
            self.groups.stage()?,
            self.streams.stage()?,
            self.docs.stage()?,
            self.held.stage()?,
        ];
        let rows = vec![self.events.stage()];
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
            O::CreateLogGroup => self.create_group(r),
            O::DeleteLogGroup => self.delete_group(r),
            O::DescribeLogGroups => self.describe_groups(r),
            O::ListLogGroups => Ok(self.list_groups(r)),
            O::CreateLogStream => self.create_stream(r),
            O::DeleteLogStream => self.delete_stream(r),
            O::DescribeLogStreams => self.describe_streams(r),
            O::PutLogEvents => self.put_events(r),
            O::GetLogEvents => self.get_events(r),
            O::FilterLogEvents => self.filter_events(r),
            O::PutRetentionPolicy => self.put_retention(r),
            O::DeleteRetentionPolicy => self.delete_retention(r),
            O::TagLogGroup | O::TagResource => self.tag(r),
            O::UntagLogGroup | O::UntagResource => self.untag(r),
            O::ListTagsLogGroup | O::ListTagsForResource => self.list_tags(r),
            O::AssociateKmsKey => self.associate_key(r),
            O::DisassociateKmsKey => self.disassociate_key(r),
            O::PutLogGroupDeletionProtection => self.set_protection(r),
            O::PutMetricFilter => self.put_metric_filter(r),
            O::DeleteMetricFilter => self.delete_metric_filter(r),
            O::DescribeMetricFilters => self.describe_metric_filters(r),
            O::TestMetricFilter => Self::test_metric_filter(r),
            O::PutSubscriptionFilter => self.put_subscription(r),
            O::DeleteSubscriptionFilter => self.delete_subscription(r),
            O::DescribeSubscriptionFilters => self.describe_subscriptions(r),
            other => self.run_docs(other, r),
        }
    }

    /// The group the request names.
    pub(crate) fn group(&self, r: &Req<'_>) -> Result<&Group, ApiError> {
        let name = r.group_name().unwrap_or_default();
        self.groups.get(&name).ok_or_else(crate::req::no_group)
    }

    pub(crate) fn group_mut(&mut self, r: &Req<'_>) -> Result<&mut Group, ApiError> {
        let name = r.group_name().unwrap_or_default();
        self.groups.get_mut(&name).ok_or_else(crate::req::no_group)
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

/// The bus the host handed this service, if it did (directly, or inside the [`crate::Ext`] a region's actor is given).
pub(crate) fn peers_of(ctx: &Ctx) -> Option<Arc<Peers>> {
    let ext = ctx.ext.as_ref()?;
    if let Ok(ours) = Arc::clone(ext).downcast::<crate::Ext>() {
        return ours.peers.clone();
    }
    Arc::clone(ext).downcast::<Peers>().ok()
}

/// The local settings in a region actor's context.
fn settings_of(ctx: &Ctx) -> crate::Settings {
    ctx.ext
        .as_ref()
        .and_then(|e| Arc::clone(e).downcast::<crate::Ext>().ok())
        .map(|e| e.settings)
        .unwrap_or_default()
}

/// Milliseconds of a duration.
fn ms(d: std::time::Duration) -> i64 {
    i64::try_from(d.as_millis()).unwrap_or(i64::MAX)
}
