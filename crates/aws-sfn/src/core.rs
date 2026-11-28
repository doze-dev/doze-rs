//! The state a Step Functions region owns, what its actor is asked, and the dispatch from an operation to the code that does it.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::Arc;

use aws_asl::{Definition, Outcome};
use aws_model_sfn::Operation;
use aws_model_sfn::tables::TABLES;
use aws_peers::{Mode, Peers};
use aws_schema::patterns::Patterns;
use aws_wire::ApiError;
use aws_wire::handler::reply_json;
use aws_wire::service::{Call, Caller, Reply};
use doze_runtime::{Clock, Ctx};
use doze_store::records::Records;
use doze_store::rows::RowTable;
use doze_store::{Ack, Db, StoreError};
use serde_json::Value;

use crate::effects::Effects;

/// A request, with the settings the service runs under (an actor may be opened again after an idle reap, so they come each time).
#[derive(Debug)]
pub(crate) struct Asked {
    pub(crate) config: Arc<crate::Config>,
    pub(crate) req: Request,
}
use crate::model::{Activity, Execution, Machine, MapRun, Run};
use crate::req::{Req, not_built};

pub(crate) const FILE: &str = "states.redb";
const MACHINES: &str = "machines";
const ACTIVITIES: &str = "activities";
pub(crate) const EXECUTIONS: &str = "executions";
const MAP_RUNS: &str = "mapruns";
pub(crate) const HISTORY: &str = "history";
/// The version of every record here.
pub(crate) const VERSION: u8 = 1;

/// What the actor is asked.
#[derive(Debug)]
pub(crate) enum Request {
    /// A call from the wire (or the peer bus).
    Call(Call),
    /// Do what is due at this time. The caller says the region and the endpoint.
    Tick(Caller, i64),
    /// The answer to a task call.
    Result {
        execution: String,
        frame: u32,
        seq: u32,
        outcome: Outcome,
    },
    /// After a restart: call again what was being called, and say when the next thing is due.
    Resume(Caller),
    /// From `doze-aws data`: what the machines hold, or remove some of it.
    Data(Box<crate::storage::DataCall>),
}

/// What the actor answers.
#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) reply: Reply,
    pub(crate) durable: Option<Ack>,
    pub(crate) next_due: Option<i64>,
    pub(crate) effects: Effects,
}

/// All of Step Functions in one region.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) machines: Records<Machine>,
    pub(crate) activities: Records<Activity>,
    /// The running executions, and the finished ones until their record is staged (then they live on disk: `finished.rs`).
    pub(crate) executions: Records<Execution>,
    /// The running executions, and each machine's finished ones by start with their summaries (kept with every `store`).
    pub(crate) index: crate::finished::ExecIndex,
    /// The executions that finished in this call, to let go of once staged.
    pub(crate) finished_now: Vec<String>,
    /// The definitions the executions run, shared.
    pub(crate) defs: crate::finished::Defs,
    pub(crate) map_runs: Records<MapRun>,
    /// History events, a row each, keyed `<execution ARN>/<id>`.
    pub(crate) history: RowTable,
    pub(crate) db: Option<Db>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) patterns: Patterns,
    /// Definitions already read, by their text.
    pub(crate) parsed: HashMap<String, Arc<Definition>>,
    /// Work on executions still to do in this call: started ones to run, answers to deliver.
    pub(crate) queue: VecDeque<crate::drive::Work>,
    /// What this call asks of the world outside the actor.
    pub(crate) effects: Effects,
    /// The last caller seen, whose region and endpoint a wake-up works as.
    pub(crate) last_caller: Option<Caller>,
    pub(crate) region: String,
    /// The local-testing settings of the last request.
    pub(crate) config: Arc<crate::Config>,
    /// Tokens already answered, newest last, with the activity ARN (empty for a callback): a second answer is `TaskTimedOut`.
    pub(crate) spent: VecDeque<(String, String)>,
}

/// Every operation of the model is built (Go's support table marks all 37 F).
pub(crate) const fn built(_op: Operation) -> bool {
    true
}

impl doze_runtime::Service for Core {
    type Request = Asked;
    type Response = Answer;
    type Error = StoreError;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, StoreError> {
        Self::open_in(&ctx.dir, Arc::clone(&ctx.clock), peers_of(ctx))
    }

    async fn handle(&mut self, asked: Asked) -> Answer {
        self.config = asked.config;
        let req = asked.req;
        let mut reply = match req {
            Request::Call(call) => self.serve(call).await,
            Request::Tick(caller, at) => {
                self.tick(&caller, at);
                Ok(aws_wire::service::Output {
                    arena: aws_schema::arena::Arena::new(),
                    root: aws_schema::arena::NIL,
                })
            }
            Request::Result {
                execution,
                frame,
                seq,
                outcome,
            } => {
                self.result(&execution, frame, seq, outcome);
                aws_wire::handler::empty()
            }
            Request::Resume(caller) => {
                self.resume(&caller);
                aws_wire::handler::empty()
            }
            Request::Data(call) => {
                let crate::storage::DataCall { region, ask, reply } = *call;
                if reply.send(self.data(&region, ask).await).is_err() {
                    // WHY: the person stopped waiting; what was asked was done all the same.
                }
                aws_wire::handler::empty()
            }
        };
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
            next_due: self.next_due(),
            effects: std::mem::take(&mut self.effects),
        }
    }

    async fn close(self) {
        if let Some(db) = self.db {
            db.close();
        }
    }
}

impl Core {
    pub(crate) fn open_in(
        dir: &Path,
        clock: Arc<dyn Clock>,
        peers: Option<Arc<Peers>>,
    ) -> Result<Self, StoreError> {
        let db = Db::open(
            dir,
            FILE,
            &[MACHINES, ACTIVITIES, EXECUTIONS, MAP_RUNS, HISTORY],
        )?;
        let read = db.read()?;
        // Only the running executions are held; the finished ones leave a summary.
        let mut index = crate::finished::ExecIndex::default();
        let executions = Records::load_where(
            EXECUTIONS,
            "execution",
            VERSION,
            &read,
            |arn, e: &Execution, len| {
                if e.status == Run::Running {
                    return true;
                }
                let mut s = crate::finished::Summary::of(e);
                s.bytes = len as u64;
                index.put_finished(arn.to_owned(), s);
                false
            },
        )?;
        let tables = Tables {
            machines: Records::load(MACHINES, "state machine", VERSION, &read)?,
            activities: Records::load(ACTIVITIES, "activity", VERSION, &read)?,
            executions,
            index,
            map_runs: Records::load(MAP_RUNS, "map run", VERSION, &read)?,
            history: RowTable::new(HISTORY, false),
        };
        drop(read);
        Ok(Self::with(clock, tables, peers, Some(db)))
    }

    fn with(clock: Arc<dyn Clock>, t: Tables, peers: Option<Arc<Peers>>, db: Option<Db>) -> Self {
        let mut index = t.index;
        let mut defs = crate::finished::Defs::default();
        let mut executions = t.executions;
        let running: Vec<String> = executions.iter().map(|(k, _)| k.to_owned()).collect();
        for arn in running {
            if let Some(e) = executions.get(&arn) {
                index.put(e);
            }
            // WHY: only to share the text in memory; the record is the same, so this stages nothing.
            if let Some(e) = executions.get_mut(&arn) {
                defs.adopt(&mut e.definition);
            }
        }
        drop(executions.stage());
        Self {
            index,
            finished_now: Vec::new(),
            defs,
            machines: t.machines,
            activities: t.activities,
            executions,
            map_runs: t.map_runs,
            history: t.history,
            db,
            clock,
            peers,
            patterns: Patterns::new(&TABLES),
            parsed: HashMap::new(),
            queue: VecDeque::new(),
            effects: Effects::default(),
            last_caller: None,
            region: String::new(),
            config: Arc::default(),
            spent: VecDeque::new(),
        }
    }

    pub(crate) fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    /// Asks the authorizer, runs the call, and writes the answer as the operation's output.
    async fn serve(&mut self, call: Call) -> Reply {
        if let Some(peers) = self.peers.clone()
            && peers.mode() != Mode::Off
            && let Err(refusal) = peers
                .authorize(&call.caller, crate::authz::needs(&call))
                .await
        {
            return Err(refusal);
        }
        self.region.clone_from(&call.caller.region);
        self.last_caller = Some(call.caller.clone());
        let Some(op) = Operation::from_index(call.op).filter(|op| built(*op)) else {
            return Err(not_built());
        };
        let r = Req::new(&call);
        let value = self.run(op, &r).await?;
        reply_json(&TABLES, &self.patterns, call.op, &value)
    }

    /// Sends what the call changed to the writer; the [`Ack`] resolves when it is on disk. The executions that finished let go of
    /// their records in memory (with a database to read them back from).
    pub(crate) fn flush(&mut self) -> Result<Option<Ack>, StoreError> {
        let changes = vec![
            self.machines.stage()?,
            self.activities.stage()?,
            self.executions.stage()?,
            self.map_runs.stage()?,
        ];
        if self.db.is_some() {
            for arn in std::mem::take(&mut self.finished_now) {
                let finished = self
                    .executions
                    .get(&arn)
                    .is_some_and(|e| e.status != Run::Running);
                if finished && let Some((_, len)) = self.executions.evict(&arn) {
                    self.index.sized(&arn, len);
                }
            }
        } else {
            self.finished_now.clear();
        }
        let rows = vec![self.history.stage()];
        Ok(self
            .db
            .as_ref()
            .and_then(|db| db.submit_with_rows(changes, rows)))
    }

    async fn run(&mut self, op: Operation, r: &Req<'_>) -> Result<Value, ApiError> {
        use Operation as O;
        match op {
            O::CreateStateMachine => self.create_machine(r).await,
            O::UpdateStateMachine => self.update_machine(r).await,
            O::DescribeStateMachine => self.describe_machine(r),
            O::DeleteStateMachine => self.delete_machine(r),
            O::ListStateMachines => self.list_machines(r),
            O::ValidateStateMachineDefinition => Ok(Self::validate(r)),
            O::DescribeStateMachineForExecution => self.machine_for_execution(r),
            O::PublishStateMachineVersion => self.publish_version(r),
            O::DeleteStateMachineVersion => self.delete_version(r),
            O::ListStateMachineVersions => self.list_versions(r),
            O::CreateStateMachineAlias => self.create_alias(r),
            O::DescribeStateMachineAlias => self.describe_alias(r),
            O::UpdateStateMachineAlias => self.update_alias(r),
            O::DeleteStateMachineAlias => self.delete_alias(r),
            O::ListStateMachineAliases => self.list_aliases(r),
            other => self.run_more(other, r),
        }
    }

    fn run_more(&mut self, op: Operation, r: &Req<'_>) -> Result<Value, ApiError> {
        use Operation as O;
        match op {
            O::StartExecution => self.start_execution(r, false),
            O::StartSyncExecution => self.start_execution(r, true),
            O::StopExecution => self.stop_execution(r),
            O::DescribeExecution => self.describe_execution(r),
            O::ListExecutions => self.list_executions(r),
            O::GetExecutionHistory => self.history_of(r),
            O::RedriveExecution => self.redrive(r),
            O::CreateActivity => self.create_activity(r),
            O::DescribeActivity => self.describe_activity(r),
            O::DeleteActivity => self.delete_activity(r),
            O::ListActivities => self.list_activities(r),
            O::GetActivityTask => self.take_activity_task(r),
            O::SendTaskSuccess => self.task_success(r),
            O::SendTaskFailure => self.task_failure(r),
            O::SendTaskHeartbeat => self.task_heartbeat(r),
            O::TagResource => self.tag_resource(r),
            O::UntagResource => self.untag_resource(r),
            O::ListTagsForResource => self.list_tags(r),
            O::DescribeMapRun => self.describe_map_run(r),
            O::ListMapRuns => self.list_map_runs(r),
            O::UpdateMapRun => self.update_map_run(r),
            // TestState runs beside the actor (lib.rs): it keeps nothing.
            _ => Err(not_built()),
        }
    }

    /// A definition read once and kept by its text.
    pub(crate) fn definition(&mut self, text: &str) -> Option<Arc<Definition>> {
        if let Some(d) = self.parsed.get(text) {
            return Some(Arc::clone(d));
        }
        let d = Arc::new(aws_asl::parse(text).ok()?);
        if self.parsed.len() > 256 {
            self.parsed.clear();
        }
        self.parsed.insert(text.to_owned(), Arc::clone(&d));
        Some(d)
    }
}

/// What `with` is given: the tables, loaded or empty.
struct Tables {
    machines: Records<Machine>,
    activities: Records<Activity>,
    executions: Records<Execution>,
    index: crate::finished::ExecIndex,
    map_runs: Records<MapRun>,
    history: RowTable,
}

/// The bus the host handed this service, if it did.
fn peers_of(ctx: &Ctx) -> Option<Arc<Peers>> {
    ctx.ext
        .as_ref()
        .and_then(|e| Arc::clone(e).downcast::<Peers>().ok())
}
