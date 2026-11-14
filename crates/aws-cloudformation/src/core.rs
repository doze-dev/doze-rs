//! The state a CloudFormation region owns (stacks, their events, change sets), and the dispatch from an operation to the code that
//! does it. Provisioning does not happen here: an operation that changes resources answers at once with the stack in progress and
//! hands back a [`Job`](crate::engine::Job) that the service runs beside the actor; the job reports what it did as
//! [`Progress`] messages.

use std::path::Path;
use std::sync::Arc;

use aws_model_cloudformation::Operation;
use aws_model_cloudformation::tables::TABLES;
use aws_peers::{Mode, Peers};
use aws_schema::patterns::Patterns;
use aws_wire::ApiError;
use aws_wire::handler::{empty, reply_json};
use aws_wire::service::{Call, Reply};
use doze_runtime::{Clock, Ctx};
use doze_store::records::Records;
use doze_store::{Ack, Db, StoreError};
use serde_json::Value;

use crate::Config;
use crate::engine::Job;
use crate::model::{ChangeSet, Event, Progress, Stack};
use crate::req::Req;

pub(crate) const FILE: &str = "cloudformation.redb";
const STACKS: &str = "stacks";
const EVENTS: &str = "events";
const CHANGE_SETS: &str = "change_sets";
const VERSION: u8 = 1;

/// What the actor is asked.
#[derive(Debug)]
pub(crate) enum Request {
    /// A call from the wire or a peer.
    Call(Call),
    /// What a running job did.
    Progress(Progress),
    /// A job asks for a nested stack.
    Child(Box<crate::nested::Ask>),
}

/// One region of CloudFormation.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) stacks: Records<Stack>,
    pub(crate) events: Records<Event>,
    pub(crate) change_sets: Records<ChangeSet>,
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) config: Arc<Config>,
    db: Option<Db>,
    patterns: Patterns,
    /// The last time handed out by `tick`.
    pub(crate) last_ms: i64,
    /// Stack events to publish to the stacks' notification topics: topic and message.
    pub(crate) outbox: Vec<(String, String)>,
    /// Jobs a call started, for the service to run once the answer is durable.
    pub(crate) started: Vec<Job>,
}

/// What the actor answers.
#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) reply: Reply,
    pub(crate) durable: Option<Ack>,
    pub(crate) jobs: Vec<Job>,
    /// For a [`Request::Child`]: the child stack's id, or why it could not be made.
    pub(crate) child: Option<Result<String, String>>,
}

/// The operations this service runs. The others are refused by name before their request is read: StackSets, drift detection, the
/// extension registry, resource scanning, generated templates, stack refactoring, Organizations access and hooks (STAY CUT in
/// SCOPE.md: there is no organization, registry or account to read locally), `CancelUpdateStack` (STAY CUT), resource import,
/// `SignalResource` (no instance can send a signal), `EstimateTemplateCost` and `DescribeAccountLimits`.
pub(crate) fn built(op: Operation) -> bool {
    use Operation as O;
    matches!(
        op,
        O::CreateStack
            | O::UpdateStack
            | O::DeleteStack
            | O::DescribeStacks
            | O::ListStacks
            | O::DescribeStackEvents
            | O::DescribeStackResource
            | O::DescribeStackResources
            | O::ListStackResources
            | O::GetTemplate
            | O::GetTemplateSummary
            | O::ValidateTemplate
            | O::CreateChangeSet
            | O::DescribeChangeSet
            | O::ExecuteChangeSet
            | O::DeleteChangeSet
            | O::ListChangeSets
            | O::ListExports
            | O::ListImports
            | O::SetStackPolicy
            | O::GetStackPolicy
            | O::UpdateTerminationProtection
            | O::RollbackStack
            | O::ContinueUpdateRollback
    )
}

impl doze_runtime::Service for Core {
    type Request = Request;
    type Response = Answer;
    type Error = StoreError;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, StoreError> {
        let mut core = Self::open_in(
            &ctx.dir,
            Arc::clone(&ctx.clock),
            peers_of(ctx),
            config_of(ctx),
        )?;
        // An operation the process stopped in the middle of cannot go on: it is marked failed, as AWS marks a stack whose
        // operation was cut off.
        core.interrupted();
        Ok(core)
    }

    async fn handle(&mut self, req: Request) -> Answer {
        let mut child = None;
        let mut reply = match req {
            Request::Call(call) => self.serve(call).await,
            Request::Progress(p) => {
                self.progress(p);
                empty()
            }
            Request::Child(ask) => {
                child = Some(self.child(*ask).await);
                empty()
            }
        };
        self.notify().await;
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
            jobs: std::mem::take(&mut self.started),
            child,
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
        config: Arc<Config>,
    ) -> Result<Self, StoreError> {
        let db = Db::open(dir, FILE, &[STACKS, EVENTS, CHANGE_SETS])?;
        let read = db.read()?;
        let stacks = Records::load(STACKS, "stack", VERSION, &read)?;
        let events = Records::load(EVENTS, "stack event", VERSION, &read)?;
        let change_sets = Records::load(CHANGE_SETS, "change set", VERSION, &read)?;
        drop(read);
        Ok(Self {
            stacks,
            events,
            change_sets,
            peers,
            clock,
            config,
            db: Some(db),
            patterns: Patterns::new(&TABLES),
            started: Vec::new(),
            last_ms: 0,
            outbox: Vec::new(),
        })
    }

    pub(crate) fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    fn flush(&mut self) -> Result<Option<Ack>, StoreError> {
        let changes = vec![
            self.stacks.stage()?,
            self.events.stage()?,
            self.change_sets.stage()?,
        ];
        Ok(self.db.as_ref().and_then(|db| db.submit(changes)))
    }

    async fn serve(&mut self, call: Call) -> Reply {
        if let Some(peers) = self.peers.clone()
            && peers.mode() != Mode::Off
            && let Err(refusal) = peers
                .authorize(&call.caller, crate::authz::needs(&call))
                .await
        {
            return Err(refusal);
        }
        let Some(op) = Operation::from_index(call.op).filter(|op| built(*op)) else {
            return Err(ApiError::new(
                501,
                "NotImplemented",
                "This operation is not built in doze-aws.",
            ));
        };
        let r = Req::new(&call);
        let value = self.dispatch(op, &r).await?;
        reply_json(&TABLES, &self.patterns, call.op, &value)
    }

    async fn dispatch(&mut self, op: Operation, r: &Req<'_>) -> Result<Value, ApiError> {
        use Operation as O;
        match op {
            O::CreateStack => self.create_stack(r).await,
            O::UpdateStack => self.update_stack(r).await,
            O::DeleteStack => self.delete_stack(r),
            O::ValidateTemplate => self.validate_template(r).await,
            O::GetTemplateSummary => self.get_template_summary(r).await,
            O::CreateChangeSet => self.create_change_set(r).await,
            O::ExecuteChangeSet => self.execute_change_set(r),
            O::RollbackStack => self.rollback_stack(r),
            O::ContinueUpdateRollback => self.continue_update_rollback(r),
            O::SetStackPolicy => self.set_stack_policy(r).await,
            other => self.read_only(other, r),
        }
    }

    fn read_only(&mut self, op: Operation, r: &Req<'_>) -> Result<Value, ApiError> {
        use Operation as O;
        match op {
            O::DescribeStacks => self.describe_stacks(r),
            O::ListStacks => Ok(self.list_stacks(r)),
            O::DescribeStackEvents => self.describe_stack_events(r),
            O::DescribeStackResource => self.describe_stack_resource(r),
            O::DescribeStackResources => self.describe_stack_resources(r),
            O::ListStackResources => self.list_stack_resources(r),
            O::GetTemplate => self.get_template(r),
            O::DescribeChangeSet => self.describe_change_set(r),
            O::DeleteChangeSet => self.delete_change_set(r),
            O::ListChangeSets => self.list_change_sets(r),
            O::ListExports => Ok(self.list_exports(r)),
            O::ListImports => self.list_imports(r),
            O::GetStackPolicy => self.get_stack_policy(r),
            O::UpdateTerminationProtection => self.update_termination_protection(r),
            _ => Err(ApiError::new(
                501,
                "NotImplemented",
                "This operation is not built in doze-aws.",
            )),
        }
    }
}

/// The bus the host handed this service, if it did.
fn peers_of(ctx: &Ctx) -> Option<Arc<Peers>> {
    ctx.ext
        .as_ref()
        .and_then(|e| Arc::clone(e).downcast::<crate::Ext>().ok())
        .map(|e| Arc::clone(&e.peers))
        .or_else(|| {
            ctx.ext
                .as_ref()
                .and_then(|e| Arc::clone(e).downcast::<Peers>().ok())
        })
}

/// The configuration the host handed this service, or the default.
fn config_of(ctx: &Ctx) -> Arc<Config> {
    ctx.ext
        .as_ref()
        .and_then(|e| Arc::clone(e).downcast::<crate::Ext>().ok())
        .map_or_else(|| Arc::new(Config::default()), |e| Arc::clone(&e.config))
}
