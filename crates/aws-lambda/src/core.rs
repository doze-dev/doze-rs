//! The state a Lambda region owns, and the dispatch from an operation to the code that does it. Management operations run here, in
//! one task that owns the state; running a function does not (it takes seconds): [`Ask::Resolve`] answers what a runner needs to
//! start one, and the runner works outside.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aws_model_lambda::Operation;
use aws_peers::{Mode, Peers};
use aws_wire::ApiError;
use aws_wire::service::Call;
use doze_runtime::{Clock, Ctx};
use doze_store::records::Records;
use doze_store::{Ack, Db, StoreError};
use serde_json::Value;

use crate::arn;
use crate::model::{Code, Function, Layer, Mapping, Signing};
use crate::req::Req;
use crate::resolve::{Resolved, Resolving};

const FILE: &str = "lambda.redb";
const FUNCTIONS: &str = "functions";
const LAYERS: &str = "layers";
const MAPPINGS: &str = "mappings";
const SIGNING: &str = "signing";
const CHECKPOINTS: &str = "checkpoints";
/// The version of every record here. A change of a record's shape is a new version with a branch that reads the old one.
const VERSION: u8 = 1;

/// What the actor is asked.
pub(crate) enum Ask {
    /// A management operation. `staged` is the code already written for it, when the operation brings some.
    Call { call: Box<Call>, staged: Staged },
    /// What a runner needs to invoke a function.
    Resolve(Box<Resolving>),
    /// Every mapping that is enabled, for starting pollers.
    Enabled,
    /// A function URL by its id: which function, and how to call it.
    Url(String),
    /// Records the position a poller has reached.
    Checkpoint { key: String, value: Option<String> },
    /// The position recorded.
    Position(String),
    /// Notes what a mapping's last invocation did.
    Result { uuid: String, result: String },
}

impl std::fmt::Debug for Ask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Ask")
    }
}

/// What the outside prepared for an operation before the actor is asked.
#[derive(Debug, Default)]
pub(crate) struct Staged {
    /// The code of `CreateFunction`, `UpdateFunctionCode` or `PublishLayerVersion`.
    pub(crate) code: Option<Code>,
    /// Whether the role named was found (and trusts Lambda); `None` when it was not looked for.
    pub(crate) role: Option<bool>,
}

/// What an operation answers.
#[derive(Debug, Default)]
pub(crate) struct Out {
    pub(crate) json: Value,
    pub(crate) status: Option<u16>,
}

impl From<Value> for Out {
    fn from(json: Value) -> Self {
        Self { json, status: None }
    }
}

/// What the actor answers.
pub(crate) enum Answer {
    Reply {
        reply: Result<Out, ApiError>,
        durable: Option<Ack>,
        retire: Vec<String>,
    },
    Resolved(Result<Box<Resolved>, ApiError>),
    Value(Option<Value>),
    Many(Vec<Value>),
}

impl std::fmt::Debug for Answer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Answer")
    }
}

/// The operations this service runs. The others are refused by name before their request is read: capacity providers and Managed
/// Instances, durable executions and function scaling are cut (SCOPE.md); `InvokeAsync` is deprecated.
pub(crate) fn built(op: Operation) -> bool {
    !matches!(
        op,
        Operation::CreateCapacityProvider
            | Operation::DeleteCapacityProvider
            | Operation::GetCapacityProvider
            | Operation::ListCapacityProviders
            | Operation::UpdateCapacityProvider
            | Operation::ListFunctionVersionsByCapacityProvider
            | Operation::PutFunctionScalingConfig
            | Operation::GetFunctionScalingConfig
            | Operation::CheckpointDurableExecution
            | Operation::GetDurableExecution
            | Operation::GetDurableExecutionHistory
            | Operation::GetDurableExecutionState
            | Operation::ListDurableExecutionsByFunction
            | Operation::SendDurableExecutionCallbackFailure
            | Operation::SendDurableExecutionCallbackHeartbeat
            | Operation::SendDurableExecutionCallbackSuccess
            | Operation::StopDurableExecution
            | Operation::InvokeAsync
    )
}

/// One region of Lambda. Owned by one task, so no operation needs a lock.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) functions: Records<Function>,
    pub(crate) layers: Records<Layer>,
    pub(crate) mappings: Records<Mapping>,
    pub(crate) signing: Records<Signing>,
    pub(crate) checkpoints: Records<String>,
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) db: Option<Db>,
    pub(crate) dir: PathBuf,
    pub(crate) region: String,
}

impl doze_runtime::Service for Core {
    type Request = Ask;
    type Response = Answer;
    type Error = StoreError;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, StoreError> {
        Self::open_in(&ctx.dir, Arc::clone(&ctx.clock), peers_of(ctx))
    }

    async fn handle(&mut self, ask: Ask) -> Answer {
        match ask {
            Ask::Call { call, staged } => self.call(*call, staged).await,
            Ask::Resolve(r) => Answer::Resolved(self.resolve(&r).await),
            Ask::Enabled => Answer::Many(self.enabled_mappings()),
            Ask::Url(id) => Answer::Value(self.url_target(&id)),
            Ask::Checkpoint { key, value } => self.checkpoint(&key, value),
            Ask::Position(key) => {
                Answer::Value(self.checkpoints.get(&key).map(|s| Value::String(s.clone())))
            }
            Ask::Result { uuid, result } => self.note_result(&uuid, result),
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
            &[FUNCTIONS, LAYERS, MAPPINGS, SIGNING, CHECKPOINTS],
        )?;
        let read = db.read()?;
        let core = Self {
            functions: Records::load(FUNCTIONS, "function", VERSION, &read)?,
            layers: Records::load(LAYERS, "layer", VERSION, &read)?,
            mappings: Records::load(MAPPINGS, "event source mapping", VERSION, &read)?,
            signing: Records::load(SIGNING, "code signing configuration", VERSION, &read)?,
            checkpoints: Records::load(CHECKPOINTS, "checkpoint", VERSION, &read)?,
            peers,
            clock,
            db: None,
            dir: dir.to_path_buf(),
            region: crate::region_of(dir),
        };
        drop(read);
        Ok(Self {
            db: Some(db),
            ..core
        })
    }

    pub(crate) fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    /// Sends what the last call changed to the writer; the [`Ack`] resolves when it is on disk.
    pub(crate) fn flush(&mut self) -> Result<Option<Ack>, StoreError> {
        let changes = vec![
            self.functions.stage()?,
            self.layers.stage()?,
            self.mappings.stage()?,
            self.signing.stage()?,
            self.checkpoints.stage()?,
        ];
        Ok(self.db.as_ref().and_then(|db| db.submit(changes)))
    }

    fn checkpoint(&mut self, key: &str, value: Option<String>) -> Answer {
        match value {
            Some(v) => self.checkpoints.insert(key.to_owned(), v),
            None => {
                self.checkpoints.remove(key);
            }
        }
        let ack = self.flush().ok().flatten();
        Answer::Reply {
            reply: Ok(Out::default()),
            durable: ack,
            retire: Vec::new(),
        }
    }

    async fn call(&mut self, call: Call, staged: Staged) -> Answer {
        if let Some(peers) = self.peers.clone()
            && peers.mode() != Mode::Off
            && let Err(refusal) = peers.authorize(&call.caller, self.needs(&call)).await
        {
            return Answer::Reply {
                reply: Err(denied(refusal)),
                durable: None,
                retire: Vec::new(),
            };
        }
        if matches!(
            Operation::from_index(call.op),
            Some(Operation::CreateEventSourceMapping | Operation::UpdateEventSourceMapping)
        ) {
            let update =
                Operation::from_index(call.op) == Some(Operation::UpdateEventSourceMapping);
            if let Err(e) = self.check_source(&Req::new(&call), update).await {
                return Answer::Reply {
                    reply: Err(e),
                    durable: None,
                    retire: Vec::new(),
                };
            }
        }
        let before = self.snapshot_names();
        let mut reply = self.dispatch(&call, &staged);
        let durable = match self.flush() {
            Ok(ack) => ack,
            Err(e) => {
                reply = Err(ApiError::internal(format!("could not save: {e}")));
                None
            }
        };
        let retire = self.changed_since(&before);
        Answer::Reply {
            reply,
            durable,
            retire,
        }
    }

    /// Every operation of the model is either run here or refused by name; the dispatch is exhaustive over the model.
    fn dispatch(&mut self, call: &Call, staged: &Staged) -> Result<Out, ApiError> {
        let Some(op) = Operation::from_index(call.op).filter(|op| built(*op)) else {
            return Err(crate::errors::refused("This operation"));
        };
        let r = Req::new(call);
        match op {
            Operation::CreateFunction => self.create_function(&r, staged),
            Operation::UpdateFunctionCode => self.update_function_code(&r, staged),
            Operation::UpdateFunctionConfiguration => {
                self.update_function_configuration(&r, staged)
            }
            Operation::GetFunction => self.get_function(&r),
            Operation::GetFunctionConfiguration => self.get_function_configuration(&r),
            Operation::ListFunctions => self.list_functions(&r),
            Operation::DeleteFunction => self.delete_function(&r),
            Operation::PublishVersion => self.publish_version(&r),
            Operation::ListVersionsByFunction => self.list_versions(&r),
            other => self.dispatch_more(other, &r, staged),
        }
    }

    fn dispatch_more(&mut self, op: Operation, r: &Req, staged: &Staged) -> Result<Out, ApiError> {
        match op {
            Operation::CreateAlias => self.create_alias(r),
            Operation::GetAlias => self.get_alias(r),
            Operation::UpdateAlias => self.update_alias(r),
            Operation::DeleteAlias => self.delete_alias(r),
            Operation::ListAliases => self.list_aliases(r),
            Operation::AddPermission => self.add_permission(r),
            Operation::RemovePermission => self.remove_permission(r),
            Operation::GetPolicy => self.get_policy(r),
            Operation::PutResourcePolicy => self.put_resource_policy(r),
            Operation::GetResourcePolicy => self.get_resource_policy(r),
            Operation::DeleteResourcePolicy => self.delete_resource_policy(r),
            Operation::CreateFunctionUrlConfig => self.create_url(r),
            Operation::GetFunctionUrlConfig => self.get_url(r),
            Operation::UpdateFunctionUrlConfig => self.update_url(r),
            Operation::DeleteFunctionUrlConfig => self.delete_url(r),
            Operation::ListFunctionUrlConfigs => self.list_urls(r),
            Operation::PublishLayerVersion => self.publish_layer(r, staged),
            other => self.dispatch_layers(other, r, staged),
        }
    }

    fn dispatch_layers(
        &mut self,
        op: Operation,
        r: &Req,
        staged: &Staged,
    ) -> Result<Out, ApiError> {
        match op {
            Operation::GetLayerVersion => self.get_layer_version(r),
            Operation::GetLayerVersionByArn => self.get_layer_version_by_arn(r),
            Operation::ListLayers => self.list_layers(r),
            Operation::ListLayerVersions => self.list_layer_versions(r),
            Operation::DeleteLayerVersion => self.delete_layer_version(r),
            Operation::AddLayerVersionPermission => self.add_layer_permission(r),
            Operation::RemoveLayerVersionPermission => self.remove_layer_permission(r),
            Operation::GetLayerVersionPolicy => self.get_layer_policy(r),
            Operation::CreateEventSourceMapping => self.create_mapping(r, staged),
            Operation::GetEventSourceMapping => self.get_mapping(r),
            Operation::UpdateEventSourceMapping => self.update_mapping(r),
            Operation::DeleteEventSourceMapping => self.delete_mapping(r),
            Operation::ListEventSourceMappings => self.list_mappings(r),
            other => self.dispatch_settings(other, r),
        }
    }

    fn dispatch_settings(&mut self, op: Operation, r: &Req) -> Result<Out, ApiError> {
        match op {
            Operation::PutFunctionConcurrency => self.put_concurrency(r),
            Operation::GetFunctionConcurrency => self.get_concurrency(r),
            Operation::DeleteFunctionConcurrency => self.delete_concurrency(r),
            Operation::PutProvisionedConcurrencyConfig => self.put_provisioned(r),
            Operation::GetProvisionedConcurrencyConfig => self.get_provisioned(r),
            Operation::DeleteProvisionedConcurrencyConfig => self.delete_provisioned(r),
            Operation::ListProvisionedConcurrencyConfigs => self.list_provisioned(r),
            Operation::PutFunctionEventInvokeConfig
            | Operation::UpdateFunctionEventInvokeConfig => {
                self.put_event_invoke(r, op == Operation::UpdateFunctionEventInvokeConfig)
            }
            Operation::GetFunctionEventInvokeConfig => self.get_event_invoke(r),
            Operation::DeleteFunctionEventInvokeConfig => self.delete_event_invoke(r),
            Operation::ListFunctionEventInvokeConfigs => self.list_event_invoke(r),
            Operation::TagResource => self.tag_resource(r),
            Operation::UntagResource => self.untag_resource(r),
            Operation::ListTags => self.list_tags(r),
            Operation::GetAccountSettings => Ok(self.account_settings(r)),
            other => self.dispatch_stored(other, r),
        }
    }

    fn dispatch_stored(&mut self, op: Operation, r: &Req) -> Result<Out, ApiError> {
        match op {
            Operation::PutRuntimeManagementConfig => self.put_extra(r, "RuntimeManagementConfig"),
            Operation::GetRuntimeManagementConfig => self.get_runtime_management(r),
            Operation::PutFunctionRecursionConfig => self.put_extra(r, "RecursiveLoop"),
            Operation::GetFunctionRecursionConfig => self.get_recursion(r),
            Operation::CreateCodeSigningConfig => self.create_signing(r),
            Operation::GetCodeSigningConfig => self.get_signing(r),
            Operation::UpdateCodeSigningConfig => self.update_signing(r),
            Operation::DeleteCodeSigningConfig => self.delete_signing(r),
            Operation::ListCodeSigningConfigs => self.list_signing(r),
            Operation::PutFunctionCodeSigningConfig => self.put_function_signing(r),
            Operation::GetFunctionCodeSigningConfig => self.get_function_signing(r),
            Operation::DeleteFunctionCodeSigningConfig => self.delete_function_signing(r),
            Operation::ListFunctionsByCodeSigningConfig => self.functions_by_signing(r),
            _ => Err(crate::errors::refused("This operation")),
        }
    }

    /// The directory the unpacked code of `code` is in.
    pub(crate) fn code_root(&self, code: &Code) -> PathBuf {
        crate::code::root(&self.dir, code)
    }

    /// The ARN of a function (or a version or alias of it).
    pub(crate) fn arn(&self, r: &Req, name: &str, qualifier: Option<&str>) -> String {
        arn::function(r.region(), r.account(), name, qualifier)
    }

    /// The code ids any record still refers to.
    pub(crate) fn referenced_code(&self) -> std::collections::BTreeSet<String> {
        let mut keep = std::collections::BTreeSet::new();
        for (_, f) in self.functions.iter() {
            keep.insert(f.latest.code.id.clone());
            keep.extend(f.versions.iter().map(|v| v.snap.code.id.clone()));
        }
        for (_, l) in self.layers.iter() {
            keep.extend(l.versions.iter().map(|v| v.code.id.clone()));
        }
        keep
    }

    /// The code directories nothing refers to any more are removed.
    pub(crate) fn sweep_code(&self) {
        crate::code::sweep(&self.dir, &self.referenced_code());
    }

    /// A name for each function and its revision, to tell which a call changed (a runner's workers for it are retired).
    fn snapshot_names(&self) -> Vec<(String, String)> {
        self.functions
            .iter()
            .map(|(n, f)| (n.to_owned(), f.latest.revision.clone()))
            .collect()
    }

    fn changed_since(&self, before: &[(String, String)]) -> Vec<String> {
        let mut out: Vec<String> = before
            .iter()
            .filter(|(n, rev)| {
                self.functions
                    .get(n)
                    .is_none_or(|f| f.latest.revision != *rev)
            })
            .map(|(n, _)| n.clone())
            .collect();
        out.sort();
        out
    }

    fn note_result(&mut self, uuid: &str, result: String) -> Answer {
        if let Some(m) = self.mappings.get_mut(uuid) {
            m.last_result = Some(result);
        }
        let ack = self.flush().ok().flatten();
        Answer::Reply {
            reply: Ok(Out::default()),
            durable: ack,
            retire: Vec::new(),
        }
    }
}

/// Lambda refuses as 403 `AccessDeniedException`, with IAM's sentence.
fn denied(refusal: ApiError) -> ApiError {
    ApiError::new(403, "AccessDeniedException", refusal.message)
}

/// The bus the host handed this service, if it did.
fn peers_of(ctx: &Ctx) -> Option<Arc<Peers>> {
    ctx.ext
        .as_ref()
        .and_then(|e| Arc::clone(e).downcast::<Peers>().ok())
}
