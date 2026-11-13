//! The state a DynamoDB service owns, and the dispatch from an operation to the code that does it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use aws_model_dynamodb::Operation;
use aws_peers::{Mode, Peers};
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};
use doze_runtime::{Clock, Ctx};
use doze_store::StoreError;
use doze_store::records::Records;
use serde_json::Value as Json;

use crate::disk::{Disk, TABLES, Writes};
use crate::meta::Table;
use crate::req::{body, not_found, refuse_legacy, table_name};
use crate::stream::{Batch, StreamInfo};

const VERSION: u8 = 1;

/// How long a table or index stays in a transitional state (`CREATING`, `UPDATING`, `DELETING`). AWS takes seconds to minutes; the
/// default here is none, so a waiter passes on its first probe, and a test that wants to see the state turns them up with
/// `DOZE_DDB_CREATE_MS`, `DOZE_DDB_UPDATE_MS` and `DOZE_DDB_DELETE_MS`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Windows {
    pub(crate) create_ms: i64,
    pub(crate) update_ms: i64,
    pub(crate) delete_ms: i64,
}

impl Windows {
    fn from_env() -> Self {
        let read = |name: &str| {
            std::env::var(name)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0)
        };
        Self {
            create_ms: read("DOZE_DDB_CREATE_MS"),
            update_ms: read("DOZE_DDB_UPDATE_MS"),
            delete_ms: read("DOZE_DDB_DELETE_MS"),
        }
    }
}

/// What the actor is asked: a call from the wire, a read of a stream by whoever consumes it, what streams there are (the Streams
/// API), or (from the timer) to delete the items whose time to live has passed.
#[derive(Debug)]
pub(crate) enum Request {
    Call(Box<Call>),
    Stream {
        stream_arn: String,
        after: u64,
        limit: usize,
    },
    Streams {
        account: String,
        region: String,
        table: Option<String>,
    },
    Tick,
}

/// What the actor answers.
#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) reply: Reply,
    /// Whether the call appended to a stream, so waiting consumers should look.
    pub(crate) appended: bool,
    pub(crate) stream: Option<Result<Batch, String>>,
    pub(crate) streams: Option<Result<Vec<StreamInfo>, String>>,
    /// When the first item with a time to live expires: the deadline the service's timer sleeps to.
    pub(crate) next_due: Option<i64>,
}

impl Answer {
    fn of(reply: Reply, appended: bool) -> Self {
        Self {
            reply,
            appended,
            stream: None,
            streams: None,
            next_due: None,
        }
    }
}

/// A call and what it asks, decoded once for the handler.
#[derive(Debug)]
pub(crate) struct Cx<'a> {
    pub(crate) call: &'a Call,
    pub(crate) body: Json,
    pub(crate) now_ms: i64,
}

impl Cx<'_> {
    pub(crate) fn region(&self) -> &str {
        &self.call.caller.region
    }

    pub(crate) fn account(&self) -> &str {
        &self.call.caller.account_id
    }

    /// The table a request names, by name or ARN.
    pub(crate) fn table_name(&self, member: &str) -> Result<String, ApiError> {
        let given = self
            .body
            .get(member)
            .and_then(Json::as_str)
            .unwrap_or_default();
        table_name(given, self.region(), self.account()).map(str::to_owned)
    }
}

/// All of DynamoDB in one region.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) disk: Disk,
    pub(crate) tables: Records<Table>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) windows: Windows,
    /// What the call in progress will write.
    pub(crate) pending: Writes,
    /// Whether the call in progress appended a stream record.
    pub(crate) appended: bool,
    /// The last sequence number of each stream, found on first use.
    pub(crate) seqs: HashMap<[u8; 16], u64>,
    /// Transaction tokens seen: the request's fingerprint and when the token stops counting.
    pub(crate) tokens: HashMap<String, (u64, i64)>,
    /// The time-to-live sweep runs at most once a second.
    pub(crate) next_sweep_ms: i64,
    /// When the first item with a time to live expires, as last read; `ttl_stale` says it must be read again (something was written).
    pub(crate) ttl_due: Option<i64>,
    pub(crate) ttl_stale: bool,
    /// Contributor Insights settings, by table (and `table/index`): stored, nothing computed.
    pub(crate) insights: Records<crate::insights::Insight>,
}

impl doze_runtime::Service for Core {
    type Request = Request;
    type Response = Answer;
    type Error = StoreError;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, StoreError> {
        Self::open_in(&ctx.dir, Arc::clone(&ctx.clock), peers_of(ctx))
    }

    async fn handle(&mut self, req: Request) -> Answer {
        let mut answer = match req {
            Request::Call(call) => self.handle_call(*call).await,
            Request::Stream {
                stream_arn,
                after,
                limit,
            } => Answer {
                stream: Some(self.read_stream(&stream_arn, after, limit)),
                ..Answer::of(Err(ApiError::internal("not a call")), false)
            },
            Request::Streams {
                account,
                region,
                table,
            } => Answer {
                streams: Some(self.list_streams(&region, &account, table.as_deref())),
                ..Answer::of(Err(ApiError::internal("not a call")), false)
            },
            Request::Tick => {
                // The timer's own sweep is not held back by the once-a-second limit a burst of calls is.
                self.next_sweep_ms = 0;
                match self.housekeeping().await {
                    Ok(appended) => Answer::of(Err(ApiError::internal("not a call")), appended),
                    Err(e) => Answer::of(Err(e), false),
                }
            }
        };
        answer.next_due = self.ttl_due();
        answer
    }

    async fn close(self) {
        self.disk.close();
    }
}

impl Core {
    pub(crate) fn open_in(
        dir: &Path,
        clock: Arc<dyn Clock>,
        peers: Option<Arc<Peers>>,
    ) -> Result<Self, StoreError> {
        let disk = Disk::open(dir)?;
        let read = disk.read()?;
        let tables = Records::load(TABLES, "table", VERSION, &read)?;
        let insights = Records::load(
            crate::disk::INSIGHTS,
            "contributor insights",
            crate::insights::VERSION,
            &read,
        )?;
        drop(read);
        Ok(Self {
            disk,
            tables,
            clock,
            peers,
            windows: Windows::from_env(),
            pending: Writes::default(),
            appended: false,
            seqs: HashMap::new(),
            tokens: HashMap::new(),
            next_sweep_ms: 0,
            ttl_due: None,
            // Read on the first answer, so a service opened by its timer or a restart finds what is due.
            ttl_stale: true,
            insights,
        })
    }

    async fn handle_call(&mut self, call: Call) -> Answer {
        if let Some(peers) = self.peers.clone()
            && peers.mode() != Mode::Off
            && let Err(refusal) = peers
                .authorize(&call.caller, crate::authz::needs(&call))
                .await
        {
            return Answer::of(Err(refusal), false);
        }
        let swept = match self.housekeeping().await {
            Ok(appended) => appended,
            Err(e) => return Answer::of(Err(e), false),
        };
        let mut answer = self.handle_checked(call).await;
        answer.appended |= swept;
        answer
    }

    async fn handle_checked(&mut self, call: Call) -> Answer {
        let json = match body(&call.body) {
            Ok(j) => j,
            Err(e) => return Answer::of(Err(e), false),
        };
        let cx = Cx {
            now_ms: self.clock.now_ms(),
            call: &call,
            body: json,
        };
        let outcome = self.dispatch(&cx);
        self.finish(outcome).await
    }

    /// Ends a call: its writes go to disk (and are waited for, so the next call reads them), and the answer is made.
    async fn finish(&mut self, outcome: Result<(Operation, Json), ApiError>) -> Answer {
        let appended = std::mem::take(&mut self.appended);
        let mut writes = std::mem::take(&mut self.pending);
        let reply = match outcome {
            Err(e) => {
                // A refused call changes nothing.
                return Answer::of(Err(e), false);
            }
            Ok((op, json)) => {
                match (self.tables.stage(), self.insights.stage()) {
                    (Ok(tables), Ok(insights)) => writes.meta.extend([tables, insights]),
                    (Err(e), _) | (_, Err(e)) => {
                        return Answer::of(
                            Err(ApiError::internal(format!("could not save: {e}"))),
                            false,
                        );
                    }
                }
                crate::answer::reply(op, &json)
            }
        };
        if !writes.is_empty() {
            self.ttl_stale = true;
        }
        if let Some(saved) = self.disk.submit(writes)
            && let Err(e) = saved.await
        {
            return Answer::of(
                Err(ApiError::internal(format!("could not save: {e}"))),
                false,
            );
        }
        Answer::of(reply, appended)
    }

    /// Every operation of the model is either run here or refused by name; the dispatch is exhaustive over those it runs.
    fn dispatch(&mut self, cx: &Cx<'_>) -> Result<(Operation, Json), ApiError> {
        let Some(op) = Operation::from_index(cx.call.op).filter(|op| built(*op)) else {
            return Err(not_built());
        };
        refuse_legacy(op, &cx.body)?;
        let json = match op {
            Operation::CreateTable => self.create_table(cx),
            Operation::DescribeTable => self.describe_table(cx),
            Operation::DeleteTable => self.delete_table(cx),
            Operation::ListTables => self.list_tables(cx),
            Operation::UpdateTable => self.update_table(cx),
            Operation::DescribeTimeToLive => self.describe_ttl(cx),
            Operation::UpdateTimeToLive => self.update_ttl(cx),
            Operation::DescribeContinuousBackups => self.describe_backups(cx),
            Operation::UpdateContinuousBackups => self.update_backups(cx),
            Operation::TagResource => self.tag_resource(cx),
            Operation::UntagResource => self.untag_resource(cx),
            Operation::ListTagsOfResource => self.list_tags(cx),
            Operation::DescribeLimits => Ok(limits()),
            Operation::DescribeEndpoints => Ok(endpoints(cx)),
            Operation::DescribeContributorInsights => self.describe_insights(cx),
            Operation::UpdateContributorInsights => self.update_insights(cx),
            Operation::ListContributorInsights => self.list_insights(cx),
            _ => self.dispatch_items(op, cx),
        }?;
        Ok((op, json))
    }

    fn dispatch_items(&mut self, op: Operation, cx: &Cx<'_>) -> Result<Json, ApiError> {
        match op {
            Operation::PutItem => self.put_item(cx),
            Operation::GetItem => self.get_item(cx),
            Operation::UpdateItem => self.update_item(cx),
            Operation::DeleteItem => self.delete_item(cx),
            Operation::Query => self.query(cx),
            Operation::Scan => self.scan(cx),
            Operation::BatchGetItem => self.batch_get(cx),
            Operation::BatchWriteItem => self.batch_write(cx),
            Operation::TransactGetItems => self.transact_get(cx),
            Operation::TransactWriteItems => self.transact_write(cx),
            Operation::ExecuteStatement => self.execute_statement(cx),
            Operation::BatchExecuteStatement => self.batch_execute_statement(cx),
            Operation::ExecuteTransaction => self.execute_transaction(cx),
            _ => Err(not_built()),
        }
    }

    /// A table by name.
    pub(crate) fn table(&self, name: &str) -> Result<Table, ApiError> {
        self.tables.get(name).cloned().ok_or_else(not_found)
    }
}

/// The operations this service runs. Every other is refused by name before its request is read.
pub(crate) fn built(op: Operation) -> bool {
    matches!(
        op,
        Operation::CreateTable
            | Operation::DescribeTable
            | Operation::DeleteTable
            | Operation::ListTables
            | Operation::UpdateTable
            | Operation::DescribeTimeToLive
            | Operation::UpdateTimeToLive
            | Operation::DescribeContinuousBackups
            | Operation::UpdateContinuousBackups
            | Operation::TagResource
            | Operation::UntagResource
            | Operation::ListTagsOfResource
            | Operation::DescribeLimits
            | Operation::DescribeEndpoints
            | Operation::PutItem
            | Operation::GetItem
            | Operation::UpdateItem
            | Operation::DeleteItem
            | Operation::Query
            | Operation::Scan
            | Operation::BatchGetItem
            | Operation::BatchWriteItem
            | Operation::TransactGetItems
            | Operation::TransactWriteItems
            | Operation::DescribeContributorInsights
            | Operation::UpdateContributorInsights
            | Operation::ListContributorInsights
            | Operation::ExecuteStatement
            | Operation::BatchExecuteStatement
            | Operation::ExecuteTransaction
    )
}

pub(crate) fn not_built() -> ApiError {
    ApiError::new(
        501,
        "NotImplemented",
        "This operation is not built in doze-aws.",
    )
}

/// The default quotas, as the developer guide gives them.
fn limits() -> Json {
    serde_json::json!({
        "AccountMaxReadCapacityUnits": 80_000,
        "AccountMaxWriteCapacityUnits": 80_000,
        "TableMaxReadCapacityUnits": 40_000,
        "TableMaxWriteCapacityUnits": 40_000,
    })
}

/// The address the caller used, since an SDK that discovers endpoints must be sent back to the same place.
fn endpoints(cx: &Cx<'_>) -> Json {
    let endpoint = &cx.call.caller.endpoint;
    let host = endpoint
        .split_once("://")
        .map_or(endpoint.as_str(), |(_, h)| h);
    serde_json::json!({"Endpoints": [{"Address": host, "CachePeriodInMinutes": 1440}]})
}

/// The bus the host handed this service, if it did.
fn peers_of(ctx: &Ctx) -> Option<Arc<Peers>> {
    ctx.ext
        .as_ref()
        .and_then(|e| Arc::clone(e).downcast::<Peers>().ok())
}
