//! The state an EventBridge region owns, and the dispatch from an operation to the code that does it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use aws_model_eventbridge::Operation;
use aws_model_eventbridge::tables::TABLES;
use aws_peers::{Mode, Peers};
use aws_schema::patterns::Patterns;
use aws_wire::ApiError;
use aws_wire::handler::{empty, reply_json};
use aws_wire::service::{Call, Caller, Reply};
use doze_runtime::{Clock, Ctx, Tasks};
use doze_store::records::Records;
use doze_store::rows::RowTable;
use doze_store::{Ack, Db, StoreError};
use serde_json::Value;

use crate::model::{ApiDestination, Archive, Bus, Connection, Replay, Rule};
use crate::pattern::Pattern;
use crate::req::{Req, not_built};
use crate::retry::Retry;

pub(crate) const FILE: &str = "eventbridge.redb";
const BUSES: &str = "buses";
const RULES: &str = "rules";
const ARCHIVES: &str = "archives";
const REPLAYS: &str = "replays";
const CONNECTIONS: &str = "connections";
const DESTINATIONS: &str = "destinations";
const EVENTS: &str = "events";
/// The version of every record here. A change of a record's shape is a new version with a branch that reads the old one.
const VERSION: u8 = 1;

/// What the actor is asked: a call from the wire, or a wake-up at a schedule's or a retry's deadline.
#[derive(Debug)]
pub(crate) enum Request {
    Call(Call),
    /// Fire what is due. The caller is whoever armed the timer: it says the region and the endpoint. The time is the deadline the
    /// timer slept to, which a service that was reaped meanwhile has forgotten.
    Tick(Caller, i64),
}

/// What the actor answers.
#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) reply: Reply,
    pub(crate) durable: Option<Ack>,
    /// The earliest time anything is due, if anything is.
    pub(crate) next_due: Option<i64>,
}

/// All of EventBridge in one region.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) buses: Records<Bus>,
    pub(crate) rules: Records<Rule>,
    pub(crate) archives: Records<Archive>,
    pub(crate) replays: Records<Replay>,
    pub(crate) connections: Records<Connection>,
    pub(crate) destinations: Records<ApiDestination>,
    /// Archived events, a row each (they can be many; they are only ever read for a replay).
    pub(crate) archived: RowTable,
    pub(crate) db: Option<Db>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) patterns: Patterns,
    /// Patterns already read, by rule key; forgotten when the rule changes.
    pub(crate) compiled: HashMap<String, Pattern>,
    /// When each scheduled rule next fires, by rule key.
    pub(crate) due: HashMap<String, i64>,
    /// Deliveries that failed and will be tried again.
    pub(crate) retries: Vec<Retry>,
    /// The last caller seen, whose endpoint and region a wake-up delivers as.
    pub(crate) last_caller: Option<Caller>,
    /// When each archive's old events were last removed, by archive key.
    pub(crate) purged: HashMap<String, i64>,
    /// Where work that must not wait on this actor is spawned (an event for another region).
    pub(crate) tasks: Tasks,
}

/// The operations this service runs. The others are refused by name before their request is read: partner event sources and global
/// endpoints (STAY CUT in SCOPE.md), and `CancelReplay` (replays here finish as they start, so there is never one to cancel).
pub(crate) fn built(op: Operation) -> bool {
    use Operation as O;
    !matches!(
        op,
        O::ActivateEventSource
            | O::DeactivateEventSource
            | O::DescribeEventSource
            | O::ListEventSources
            | O::CreatePartnerEventSource
            | O::DeletePartnerEventSource
            | O::DescribePartnerEventSource
            | O::ListPartnerEventSources
            | O::ListPartnerEventSourceAccounts
            | O::PutPartnerEvents
            | O::CreateEndpoint
            | O::DeleteEndpoint
            | O::DescribeEndpoint
            | O::ListEndpoints
            | O::UpdateEndpoint
            | O::CancelReplay
    )
}

impl doze_runtime::Service for Core {
    type Request = Request;
    type Response = Answer;
    type Error = StoreError;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, StoreError> {
        Self::open_in(
            &ctx.dir,
            Arc::clone(&ctx.clock),
            peers_of(ctx),
            ctx.tasks.clone(),
        )
    }

    async fn handle(&mut self, req: Request) -> Answer {
        let mut reply = match req {
            Request::Call(call) => self.serve(call).await,
            Request::Tick(caller, at) => {
                self.tick(&caller, at).await;
                empty()
            }
        };
        let durable = match self.flush() {
            Ok(ack) => ack,
            Err(e) => {
                reply = Err(ApiError::internal(format!("could not save: {e}")));
                None
            }
        };
        let next_due = self.next_due();
        Answer {
            reply,
            durable,
            next_due,
        }
    }

    async fn close(self) {
        if let Some(db) = self.db {
            db.close();
        }
    }
}

impl Core {
    /// EventBridge with no disk (the rules alone are under test).
    #[cfg(test)]
    pub(crate) fn in_memory(clock: Arc<dyn Clock>, peers: Option<Arc<Peers>>) -> Self {
        Self::with(clock, Tables::empty(), peers, None, Tasks::new())
    }

    pub(crate) fn open_in(
        dir: &Path,
        clock: Arc<dyn Clock>,
        peers: Option<Arc<Peers>>,
        tasks: Tasks,
    ) -> Result<Self, StoreError> {
        let db = Db::open(
            dir,
            FILE,
            &[
                BUSES,
                RULES,
                ARCHIVES,
                REPLAYS,
                CONNECTIONS,
                DESTINATIONS,
                EVENTS,
            ],
        )?;
        let read = db.read()?;
        let tables = Tables {
            buses: Records::load(BUSES, "event bus", VERSION, &read)?,
            rules: Records::load(RULES, "rule", VERSION, &read)?,
            archives: Records::load(ARCHIVES, "archive", VERSION, &read)?,
            replays: Records::load(REPLAYS, "replay", VERSION, &read)?,
            connections: Records::load(CONNECTIONS, "connection", VERSION, &read)?,
            destinations: Records::load(DESTINATIONS, "API destination", VERSION, &read)?,
            archived: RowTable::new(EVENTS, false),
        };
        drop(read);
        Ok(Self::with(clock, tables, peers, Some(db), tasks))
    }

    fn with(
        clock: Arc<dyn Clock>,
        t: Tables,
        peers: Option<Arc<Peers>>,
        db: Option<Db>,
        tasks: Tasks,
    ) -> Self {
        Self {
            buses: t.buses,
            rules: t.rules,
            archives: t.archives,
            replays: t.replays,
            connections: t.connections,
            destinations: t.destinations,
            archived: t.archived,
            db,
            clock,
            peers,
            patterns: Patterns::new(&TABLES),
            compiled: HashMap::new(),
            due: HashMap::new(),
            retries: Vec::new(),
            last_caller: None,
            purged: HashMap::new(),
            tasks,
        }
    }

    pub(crate) fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    /// Asks the authorizer, runs the call, and writes the answer as the operation's output.
    async fn serve(&mut self, call: Call) -> Reply {
        if let Some(peers) = self.peers.clone()
            && peers.mode() != Mode::Off
            && let Err(refusal) = peers.authorize(&call.caller, self.needs(&call)).await
        {
            return Err(refusal);
        }
        self.last_caller = Some(call.caller.clone());
        self.dispatch(&call).await
    }

    /// Sends what the last call changed to the writer; the [`Ack`] resolves when it is on disk.
    pub(crate) fn flush(&mut self) -> Result<Option<Ack>, StoreError> {
        let changes = vec![
            self.buses.stage()?,
            self.rules.stage()?,
            self.archives.stage()?,
            self.replays.stage()?,
            self.connections.stage()?,
            self.destinations.stage()?,
        ];
        let rows = vec![self.archived.stage()];
        Ok(self
            .db
            .as_ref()
            .and_then(|db| db.submit_with_rows(changes, rows)))
    }

    /// Every operation of the model is either run here or refused by name; the dispatch is exhaustive over the model.
    pub(crate) async fn dispatch(&mut self, call: &Call) -> Reply {
        let Some(op) = Operation::from_index(call.op).filter(|op| built(*op)) else {
            return Err(not_built());
        };
        self.ensure_default(&call.caller.account_id, &call.caller.region);
        let r = Req::new(call);
        let value = match op {
            Operation::PutEvents => self.put_events(&r).await?,
            Operation::StartReplay => self.start_replay(&r).await?,
            other => self.run(other, &r)?,
        };
        reply_json(&TABLES, &self.patterns, call.op, &value)
    }

    fn run(&mut self, op: Operation, r: &Req<'_>) -> Result<Value, ApiError> {
        use Operation as O;
        match op {
            O::CreateEventBus => self.create_bus(r),
            O::DeleteEventBus => self.delete_bus(r),
            O::DescribeEventBus => self.describe_bus(r),
            O::ListEventBuses => self.list_buses(r),
            O::UpdateEventBus => self.update_bus(r),
            O::PutPermission => self.put_permission(r),
            O::RemovePermission => self.remove_permission(r),
            O::PutRule => self.put_rule(r),
            O::DeleteRule => self.delete_rule(r),
            O::DescribeRule => self.describe_rule(r),
            O::ListRules => self.list_rules(r),
            O::EnableRule => self.set_enabled(r, true),
            O::DisableRule => self.set_enabled(r, false),
            O::PutTargets => self.put_targets(r),
            O::RemoveTargets => self.remove_targets(r),
            O::ListTargetsByRule => self.list_targets(r),
            O::ListRuleNamesByTarget => self.list_rule_names_by_target(r),
            O::TestEventPattern => Self::test_event_pattern(r),
            O::TagResource => self.tag_resource(r),
            O::UntagResource => self.untag_resource(r),
            O::ListTagsForResource => self.list_tags(r),
            other => self.run_more(other, r),
        }
    }

    fn run_more(&mut self, op: Operation, r: &Req<'_>) -> Result<Value, ApiError> {
        use Operation as O;
        match op {
            O::CreateArchive => self.create_archive(r),
            O::DescribeArchive => self.describe_archive(r),
            O::ListArchives => self.list_archives(r),
            O::UpdateArchive => self.update_archive(r),
            O::DeleteArchive => self.delete_archive(r),
            O::DescribeReplay => self.describe_replay(r),
            O::ListReplays => self.list_replays(r),
            O::CreateConnection => self.create_connection(r),
            O::UpdateConnection => self.update_connection(r),
            O::DeauthorizeConnection => self.deauthorize_connection(r),
            O::DeleteConnection => self.delete_connection(r),
            O::DescribeConnection => self.describe_connection(r),
            O::ListConnections => self.list_connections(r),
            O::CreateApiDestination => self.create_destination(r),
            O::UpdateApiDestination => self.update_destination(r),
            O::DeleteApiDestination => self.delete_destination(r),
            O::DescribeApiDestination => self.describe_destination(r),
            O::ListApiDestinations => self.list_destinations(r),
            // PutEvents and StartReplay are run by `dispatch`, and the rest are refused by name before they get here.
            _ => Err(not_built()),
        }
    }
}

/// What `with` is given: the tables, loaded or empty.
struct Tables {
    buses: Records<Bus>,
    rules: Records<Rule>,
    archives: Records<Archive>,
    replays: Records<Replay>,
    connections: Records<Connection>,
    destinations: Records<ApiDestination>,
    archived: RowTable,
}

impl Tables {
    #[cfg(test)]
    fn empty() -> Self {
        Self {
            buses: Records::new(BUSES, "event bus", VERSION),
            rules: Records::new(RULES, "rule", VERSION),
            archives: Records::new(ARCHIVES, "archive", VERSION),
            replays: Records::new(REPLAYS, "replay", VERSION),
            connections: Records::new(CONNECTIONS, "connection", VERSION),
            destinations: Records::new(DESTINATIONS, "API destination", VERSION),
            archived: RowTable::new(EVENTS, true),
        }
    }
}

/// The bus the host handed this service, if it did.
fn peers_of(ctx: &Ctx) -> Option<Arc<Peers>> {
    ctx.ext
        .as_ref()
        .and_then(|e| Arc::clone(e).downcast::<Peers>().ok())
}
