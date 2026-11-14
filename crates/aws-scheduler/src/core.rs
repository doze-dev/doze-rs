//! The state a Scheduler region owns, and the dispatch from an operation to the code that does it.

use std::path::Path;
use std::sync::Arc;

use aws_model_scheduler::Operation;
use aws_peers::{Mode, Peers};
use aws_wire::ApiError;
use aws_wire::service::Call;
use doze_runtime::{Clock, Ctx};
use doze_store::records::Records;
use doze_store::{Ack, Db, StoreError};
use serde_json::Value;

use crate::fire::Invocation;
use crate::model::{Group, Schedule};
use crate::req::{Req, not_built};

pub(crate) const FILE: &str = "scheduler.redb";
const GROUPS: &str = "groups";
const SCHEDULES: &str = "schedules";
/// The version of every record here. A change of a record's shape is a new version with a branch that reads the old one.
const VERSION: u8 = 1;

/// What the actor is asked.
#[derive(Debug)]
pub(crate) enum Request {
    Call(Box<Call>),
    /// Fire what is due (the region's timer woke).
    Tick,
    /// After a restart: re-plan recurring schedules whose time passed while the process was down (AWS does not replay what it
    /// missed), and fire one-time schedules that are overdue (they would otherwise never fire).
    Resume,
    /// The local hook: fire this schedule now.
    Fire {
        account: Option<String>,
        group: String,
        name: String,
    },
}

/// What the actor answers.
#[derive(Debug)]
pub(crate) struct Answer {
    /// The operation's output as JSON (`null` for none).
    pub(crate) reply: Result<Value, ApiError>,
    pub(crate) durable: Option<Ack>,
    /// The earliest time a schedule fires, if any does.
    pub(crate) next_due: Option<i64>,
    /// Invocations to make, off the actor.
    pub(crate) fired: Vec<Invocation>,
}

/// All of Scheduler in one region.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) groups: Records<Group>,
    pub(crate) schedules: Records<Schedule>,
    pub(crate) db: Option<Db>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) region: String,
    /// What the current request fired.
    pub(crate) fired: Vec<Invocation>,
}

/// Every operation of the model is built.
pub(crate) fn built(op: Operation) -> bool {
    Operation::ALL.contains(&op)
}

impl doze_runtime::Service for Core {
    type Request = Request;
    type Response = Answer;
    type Error = StoreError;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, StoreError> {
        Self::open_in(&ctx.dir, Arc::clone(&ctx.clock), peers_of(ctx))
    }

    async fn handle(&mut self, req: Request) -> Answer {
        let mut reply = match req {
            Request::Call(call) => self.serve(&call).await,
            Request::Tick => {
                self.tick();
                Ok(Value::Null)
            }
            Request::Resume => {
                self.resume();
                Ok(Value::Null)
            }
            Request::Fire {
                account,
                group,
                name,
            } => self.fire_now(account.as_deref(), &group, &name),
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
            fired: std::mem::take(&mut self.fired),
        }
    }

    async fn close(self) {
        if let Some(db) = self.db {
            db.close();
        }
    }
}

impl Core {
    /// Scheduler with no disk.
    #[cfg(test)]
    pub(crate) fn in_memory(clock: Arc<dyn Clock>, peers: Option<Arc<Peers>>) -> Self {
        Self::with(
            clock,
            Records::new(GROUPS, "schedule group", VERSION),
            Records::new(SCHEDULES, "schedule", VERSION),
            peers,
            None,
            "us-east-1".to_owned(),
        )
    }

    pub(crate) fn open_in(
        dir: &Path,
        clock: Arc<dyn Clock>,
        peers: Option<Arc<Peers>>,
    ) -> Result<Self, StoreError> {
        let db = Db::open(dir, FILE, &[GROUPS, SCHEDULES])?;
        let read = db.read()?;
        let groups = Records::load(GROUPS, "schedule group", VERSION, &read)?;
        let schedules = Records::load(SCHEDULES, "schedule", VERSION, &read)?;
        drop(read);
        Ok(Self::with(
            clock,
            groups,
            schedules,
            peers,
            Some(db),
            crate::region_of(dir),
        ))
    }

    fn with(
        clock: Arc<dyn Clock>,
        groups: Records<Group>,
        schedules: Records<Schedule>,
        peers: Option<Arc<Peers>>,
        db: Option<Db>,
        region: String,
    ) -> Self {
        Self {
            groups,
            schedules,
            db,
            clock,
            peers,
            region,
            fired: Vec::new(),
        }
    }

    pub(crate) fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    /// Asks the authorizer, runs the call, and writes the answer as the operation's output.
    async fn serve(&mut self, call: &Call) -> Result<Value, ApiError> {
        if let Some(peers) = self.peers.clone()
            && peers.mode() != Mode::Off
            && let Err(refusal) = peers.authorize(&call.caller, self.needs(call)).await
        {
            return Err(refusal);
        }
        self.dispatch(call)
    }

    /// Sends what the last call changed to the writer; the [`Ack`] resolves when it is on disk.
    pub(crate) fn flush(&mut self) -> Result<Option<Ack>, StoreError> {
        let changes = vec![self.groups.stage()?, self.schedules.stage()?];
        Ok(self.db.as_ref().and_then(|db| db.submit(changes)))
    }

    /// Every operation of the model is run here; the dispatch is exhaustive over the model.
    pub(crate) fn dispatch(&mut self, call: &Call) -> Result<Value, ApiError> {
        let Some(op) = Operation::from_index(call.op).filter(|op| built(*op)) else {
            return Err(not_built());
        };
        self.ensure_default(&call.caller.account_id);
        let r = Req::new(call);
        self.run(op, &r)
    }

    fn run(&mut self, op: Operation, r: &Req<'_>) -> Result<Value, ApiError> {
        use Operation as O;
        match op {
            O::CreateSchedule => self.create_schedule(r),
            O::UpdateSchedule => self.update_schedule(r),
            O::GetSchedule => self.get_schedule(r),
            O::DeleteSchedule => self.delete_schedule(r),
            O::ListSchedules => self.list_schedules(r),
            O::CreateScheduleGroup => self.create_group(r),
            O::GetScheduleGroup => self.get_group(r),
            O::DeleteScheduleGroup => self.delete_group(r),
            O::ListScheduleGroups => self.list_groups(r),
            O::TagResource => self.tag_resource(r),
            O::UntagResource => self.untag_resource(r),
            O::ListTagsForResource => self.list_tags(r),
        }
    }
}

/// The bus the host handed this service, if it did.
fn peers_of(ctx: &Ctx) -> Option<Arc<Peers>> {
    ctx.ext
        .as_ref()
        .and_then(|e| Arc::clone(e).downcast::<Peers>().ok())
}
