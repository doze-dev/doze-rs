//! The state a CloudWatch Logs service owns, and how the actor answers what it is asked (the dispatch from an operation to the code
//! that does it is `dispatch`).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use aws_model_cloudwatch_logs::tables::TABLES;
use aws_peers::{Mode, Peers};
use aws_schema::patterns::Patterns;
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};
use doze_runtime::{Clock, Ctx, Tasks};
use doze_store::records::Records;
use doze_store::rows::RowTable;
use doze_store::{Ack, Db, StoreError};
use serde_json::Value;

use crate::model::{Doc, Group, Stream};
use crate::queries::Registry;

pub(crate) const FILE: &str = "logs.redb";
const GROUPS: &str = "groups";
const STREAMS: &str = "streams";
const DOCS: &str = "docs";
pub(crate) const EVENTS: &str = "events";
const HELD: &str = "held";
/// The version of every record here but events (`model::Event`). A change of a record's shape is a new version with a branch that
/// reads the old one.
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
    /// The process's Insights queries (kept outside the actor, which is reaped when idle).
    pub(crate) queries: Arc<Registry>,
    /// Where query runs and Live Tail sessions are started; `None` under test, where a query runs in line.
    pub(crate) tasks: Option<Tasks>,
    /// The open Live Tail sessions.
    pub(crate) tails: Vec<crate::tail::Tail>,
    /// The export the last call made, run before it is answered.
    pub(crate) export: Option<crate::export::Pending>,
}

/// What the actor answers: the reply, the commit it must wait for, and when its scheduled queries next need it.
#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) reply: Reply,
    pub(crate) durable: Option<Ack>,
    pub(crate) next_due: Option<i64>,
}

/// What the actor is asked: a call from the wire, a data question from `doze-aws data`, a Live Tail session to open, or the
/// scheduled queries that are due to be run.
#[derive(Debug)]
pub(crate) enum Ask {
    Call(Box<Call>),
    Data(Box<crate::storage::Asked>),
    Tail(Box<crate::tail::TailAsk>),
    Tick,
}

impl Answer {
    fn bare(reply: Reply, next_due: Option<i64>) -> Self {
        Self {
            reply,
            durable: None,
            next_due,
        }
    }
}

impl doze_runtime::Service for Core {
    type Request = Ask;
    type Response = Answer;
    type Error = StoreError;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, StoreError> {
        let mut core = Self::open_in(&ctx.dir, Arc::clone(&ctx.clock), peers_of(ctx))?;
        core.set(settings_of(ctx));
        core.queries = queries_of(ctx);
        core.tasks = Some(ctx.tasks.clone());
        Ok(core)
    }

    async fn handle(&mut self, ask: Ask) -> Answer {
        let call = match ask {
            Ask::Call(call) => *call,
            Ask::Data(asked) => {
                let crate::storage::Asked { region, ask, reply } = *asked;
                if reply.send(self.data(&region, ask)).is_err() {
                    // WHY: the person stopped waiting; what was asked was done all the same.
                }
                return Answer::bare(Err(ApiError::internal("not a call")), self.next_due());
            }
            Ask::Tail(t) => return self.handle_tail(*t).await,
            Ask::Tick => {
                let next_due = self.tick();
                let durable = self.flush().ok().flatten();
                return Answer {
                    reply: Err(ApiError::internal("not a call")),
                    durable,
                    next_due,
                };
            }
        };
        if let Err(refusal) = self.authorized(&call).await {
            return Answer::bare(Err(refusal), self.next_due());
        }
        let mut reply = self.dispatch(&call);
        self.run_export(&call, &mut reply).await;
        let durable = match self.flush() {
            Ok(ack) => ack,
            Err(e) => {
                reply = Err(ApiError::internal(format!("could not save: {e}")));
                None
            }
        };
        self.send_metrics(&call).await;
        self.send_forwards(&call).await;
        Answer {
            reply,
            durable,
            next_due: self.next_due(),
        }
    }

    async fn close(self) {
        if let Some(db) = self.db {
            db.close();
        }
    }
}

impl Core {
    /// The authorizer's answer, when IAM is on.
    async fn authorized(&self, call: &Call) -> Result<(), ApiError> {
        match self.peers.clone() {
            Some(peers) if peers.mode() != Mode::Off => {
                peers
                    .authorize(&call.caller, crate::authz::needs(call))
                    .await
            }
            _ => Ok(()),
        }
    }

    async fn handle_tail(&mut self, t: crate::tail::TailAsk) -> Answer {
        let crate::tail::TailAsk {
            call,
            events,
            reply,
        } = t;
        let opened = match self.authorized(&call).await {
            Ok(()) => self.open_tail(&call, events),
            Err(refusal) => Err(refusal),
        };
        if reply.send(opened).is_err() {
            // WHY: the caller went away before the session opened; the next write drops it.
        }
        Answer::bare(Err(ApiError::internal("not a call")), self.next_due())
    }

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
            queries: Arc::new(Registry::default()),
            tasks: None,
            tails: Vec::new(),
            export: None,
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

fn ext_of(ctx: &Ctx) -> Option<Arc<crate::Ext>> {
    ctx.ext
        .as_ref()
        .and_then(|e| Arc::clone(e).downcast::<crate::Ext>().ok())
}

/// The local settings in a region actor's context.
fn settings_of(ctx: &Ctx) -> crate::Settings {
    ext_of(ctx).map(|e| e.settings).unwrap_or_default()
}

/// The process's queries, from a region actor's context (a new registry for a context that has none).
fn queries_of(ctx: &Ctx) -> Arc<Registry> {
    ext_of(ctx).map_or_else(|| Arc::new(Registry::default()), |e| Arc::clone(&e.queries))
}

/// Milliseconds of a duration.
fn ms(d: std::time::Duration) -> i64 {
    i64::try_from(d.as_millis()).unwrap_or(i64::MAX)
}
