//! The state a DynamoDB service owns, and the dispatch from an operation to the code that does it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use aws_model_dynamodb::Operation;
use aws_peers::{Mode, Peers};
use aws_wire::ApiError;
use aws_wire::service::{Call, Output, Reply};
use doze_runtime::{Clock, Ctx};
use doze_store::records::{Changes, Records};
use doze_store::{Ack, StoreError};
use serde_json::Value as Json;

use crate::disk::{Disk, TABLES, Writes};
use crate::extras::Extra;
use crate::meta::Table;
use crate::req::{body, not_found, table_name};
pub(crate) use crate::route::{built, not_built};
use crate::route::{endpoints, limits};
use crate::stream::{Batch, StreamInfo};
use crate::work::{Done, Work};

const VERSION: u8 = 1;

pub(crate) use crate::windows::Ext;
pub use crate::windows::Windows;

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
    /// From the console's Lab: the time-to-live sweep of a table now (`forced`).
    Sweep {
        table: String,
    },
    /// From `doze-aws data`: what the tables hold, or remove some of it.
    Data(Box<crate::storage::Asked>),
    /// What work done outside the actor found (`work.rs`).
    Done(Box<Done>),
    /// The resource policy that governs an ARN (for the Streams API).
    Policy(String),
}

/// What the actor answers.
#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) reply: Reply,
    /// Whether the call appended to a stream, so waiting consumers should look.
    pub(crate) appended: bool,
    /// The streams appended to, each as its ARN's resource (`table/<name>/stream/<label>`): only their consumers are woken.
    pub(crate) grown: Vec<String>,
    pub(crate) stream: Option<Result<Batch, String>>,
    pub(crate) streams: Option<Result<Vec<StreamInfo>, String>>,
    /// When the first item with a time to live expires: the deadline the service's timer sleeps to.
    pub(crate) next_due: Option<i64>,
    /// What is left to do outside the actor before the caller is answered.
    pub(crate) work: Vec<Work>,
    /// The policy a `Policy` request asked for.
    pub(crate) policy: Option<String>,
    /// The commits of what the request wrote: the caller is answered once they are on disk ([`durable`]). The actor does not wait
    /// for them: the next call reads the queued writes ([`doze_store::pending`]).
    pub(crate) durable: Vec<Ack>,
    /// What a forced sweep came to.
    pub(crate) swept: Option<Result<crate::forced::Sweep, ApiError>>,
}

impl Answer {
    pub(crate) fn of(reply: Reply, appended: bool) -> Self {
        Self {
            reply,
            appended,
            grown: Vec::new(),
            stream: None,
            streams: None,
            next_due: None,
            work: Vec::new(),
            policy: None,
            swept: None,
            durable: Vec::new(),
        }
    }
}

/// Waits for the commits of an answer; the first that failed is the caller's answer.
pub(crate) async fn durable(acks: Vec<Ack>) -> Result<(), ApiError> {
    for ack in acks {
        ack.await
            .map_err(|e| ApiError::internal(format!("could not save: {e}")))?;
    }
    Ok(())
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
    /// The streams the request in progress appended to (see [`Answer::grown`]).
    pub(crate) grown: std::collections::BTreeSet<String>,
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
    /// Backups, export and import tasks, resource policies, Kinesis streaming destinations.
    pub(crate) extras: Records<Extra>,
    /// What the call in progress leaves to be done outside the actor.
    pub(crate) work: Vec<Work>,
    /// Orders the recovery log's entries written in one millisecond.
    pub(crate) pitr_seq: u32,
    /// The commits the request in progress queued (see [`Answer::durable`]).
    pub(crate) durable: Vec<Ack>,
}

impl doze_runtime::Service for Core {
    type Request = Request;
    type Response = Answer;
    type Error = StoreError;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, StoreError> {
        let mut core = Self::open_in(&ctx.dir, Arc::clone(&ctx.clock), peers_of(ctx))?;
        if let Some(ext) = ctx
            .ext
            .as_ref()
            .and_then(|e| Arc::clone(e).downcast::<Ext>().ok())
        {
            core.windows = ext.windows;
        }
        Ok(core)
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
            Request::Data(asked) => {
                let crate::storage::Asked { region, ask, reply } = *asked;
                if reply.send(self.data(&region, ask).await).is_err() {
                    // WHY: the person stopped waiting; what was asked was done all the same.
                }
                // What was emptied may have held the item the timer sleeps to.
                self.ttl_stale = true;
                Answer::of(Err(ApiError::internal("not a call")), false)
            }
            Request::Tick => {
                // The timer's own sweep is not held back by the once-a-second limit a burst of calls is.
                self.next_sweep_ms = 0;
                let mut answer = match self.housekeeping().await {
                    Ok(appended) => Answer::of(Err(ApiError::internal("not a call")), appended),
                    Err(e) => Answer::of(Err(e), false),
                };
                answer.work = std::mem::take(&mut self.work);
                answer
            }
            Request::Sweep { table } => self.sweep_answer(&table).await,
            Request::Policy(arn) => Answer {
                policy: self.policy_text(&arn),
                ..Answer::of(Err(ApiError::internal("not a call")), false)
            },
            Request::Done(done) => {
                let applied = self.apply_done(*done, self.clock.now_ms());
                let appended = std::mem::take(&mut self.appended);
                let work = std::mem::take(&mut self.work);
                let saved = match applied {
                    Ok(()) => self.save().await,
                    Err(e) => {
                        self.pending = Writes::default();
                        Err(e)
                    }
                };
                let empty = || Output {
                    arena: aws_schema::arena::Arena::new(),
                    root: aws_schema::arena::NIL,
                };
                let mut answer = Answer::of(saved.map(|()| empty()), appended);
                answer.work = work;
                answer
            }
        };
        answer.next_due = self.ttl_due();
        answer.grown = std::mem::take(&mut self.grown).into_iter().collect();
        answer.durable = std::mem::take(&mut self.durable);
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
        let extras = Records::load(
            crate::disk::EXTRAS,
            "dynamodb record",
            crate::extras::VERSION,
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
            grown: std::collections::BTreeSet::new(),
            seqs: HashMap::new(),
            tokens: HashMap::new(),
            next_sweep_ms: 0,
            ttl_due: None,
            // Read on the first answer, so a service opened by its timer or a restart finds what is due.
            ttl_stale: true,
            insights,
            extras,
            work: Vec::new(),
            pitr_seq: 0,
            durable: Vec::new(),
        })
    }

    async fn handle_call(&mut self, call: Call) -> Answer {
        if let Some(peers) = self.peers.clone()
            && peers.mode() != Mode::Off
            && let Err(refusal) = peers.authorize(&call.caller, self.needs(&call)).await
        {
            return Answer::of(Err(refusal), false);
        }
        let swept = match self.housekeeping().await {
            Ok(appended) => appended,
            Err(e) => return Answer::of(Err(e), false),
        };
        // What the sweep left to send goes whether or not the call is refused.
        let swept_work = std::mem::take(&mut self.work);
        let mut answer = self.handle_checked(call).await;
        answer.appended |= swept;
        answer.work.splice(0..0, swept_work);
        answer
    }

    async fn handle_checked(&mut self, call: Call) -> Answer {
        let json = match body(&call.body).and_then(|j| match Operation::from_index(call.op) {
            // The legacy parameters become the expressions they stand for (`legacy.rs`).
            Some(op) => crate::legacy::translate(op, j),
            None => Ok(j),
        }) {
            Ok(j) => j,
            Err(e) => return Answer::of(Err(e), false),
        };
        let cx = Cx {
            now_ms: self.clock.now_ms(),
            call: &call,
            body: json,
        };
        self.count_ahead(&cx).await;
        let outcome = self.dispatch(&cx);
        self.finish(outcome).await
    }

    /// Ends a call: its writes are queued for the disk (the caller's answer waits for them, the next call reads them queued), and
    /// the answer is made.
    async fn finish(&mut self, outcome: Result<(Operation, Json), ApiError>) -> Answer {
        let appended = std::mem::take(&mut self.appended);
        let work = std::mem::take(&mut self.work);
        let (op, json) = match outcome {
            Err(e) => {
                // A refused call changes nothing.
                self.pending = Writes::default();
                return Answer::of(Err(e), false);
            }
            Ok(done) => done,
        };
        if let Err(e) = self.save().await {
            return Answer::of(Err(e), false);
        }
        let mut answer = Answer::of(crate::answer::reply(op, &json), appended);
        answer.work = work;
        answer
    }

    /// The records every call may change, as rows for the writer.
    pub(crate) fn stage_meta(&mut self) -> Result<Vec<Changes>, StoreError> {
        Ok(vec![
            self.tables.stage()?,
            self.insights.stage()?,
            self.extras.stage()?,
        ])
    }

    /// Queues what the call changed. Its commit is waited for outside the actor, by the caller's answer ([`Answer::durable`]); a
    /// write with work inside the transaction (a backup's or a restore's copy), which a read cannot see queued, is waited for here.
    pub(crate) async fn save(&mut self) -> Result<(), ApiError> {
        let mut writes = std::mem::take(&mut self.pending);
        writes.meta.extend(
            self.stage_meta()
                .map_err(|e| ApiError::internal(format!("could not save: {e}")))?,
        );
        if !writes.is_empty() {
            self.ttl_stale = true;
        }
        let unseen = !writes.ops.is_empty();
        let Some(saved) = self.disk.submit(writes) else {
            return Ok(());
        };
        if unseen {
            return durable(vec![saved]).await;
        }
        self.durable.push(saved);
        Ok(())
    }

    /// Every operation of the model is either run here or refused by name; the dispatch is exhaustive over those it runs.
    fn dispatch(&mut self, cx: &Cx<'_>) -> Result<(Operation, Json), ApiError> {
        let Some(op) = Operation::from_index(cx.call.op).filter(|op| built(*op)) else {
            return Err(not_built());
        };
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
            _ => self.dispatch_more(op, cx),
        }?;
        Ok((op, json))
    }

    /// A table by name.
    pub(crate) fn table(&self, name: &str) -> Result<Table, ApiError> {
        self.tables.get(name).cloned().ok_or_else(not_found)
    }
}

/// The bus the host handed this service, if it did (directly, or inside the [`Ext`] that also carries the windows).
pub(crate) fn peers_of(ctx: &Ctx) -> Option<Arc<Peers>> {
    let ext = ctx.ext.as_ref()?;
    if let Ok(ours) = Arc::clone(ext).downcast::<Ext>() {
        return ours.peers.clone();
    }
    Arc::clone(ext).downcast::<Peers>().ok()
}
