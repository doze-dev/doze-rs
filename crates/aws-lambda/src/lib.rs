//! Lambda: functions run as supervised host processes that speak the Lambda Runtime API (no containers, no images), with versions,
//! aliases, layers, function URLs, resource policies, reserved concurrency, event source mappings for SQS, Kinesis and DynamoDB
//! streams, asynchronous invocation with destinations, response streaming, and logs written to CloudWatch Logs in the shape AWS
//! writes them. A function runs as its execution role: its process holds an STS session that IAM resolves.
//!
//! Management operations run in a state machine behind a lazy actor per region ([`core`]); running a function does not (it takes
//! seconds) and happens beside the actor ([`invoke`], [`rt`]). Event source pollers wait for the source to say it has something
//! (`poll`) and cost nothing when no mapping is enabled.
//!
//! **Stored and flagged, not acted on:** provisioned concurrency, code signing, runtime management, recursion detection, SnapStart,
//! VPC, file systems, tracing, KMS keys. **Refused by name:** capacity providers, durable executions, function scaling, `InvokeAsync`.
//! A container-image function is stored and listed; invoking it says why it cannot run.

mod aliases;
mod arn;
mod async_invoke;
mod authz;
mod code;
mod config;
mod core;
mod errors;
mod eventstream;
mod functions;
mod invoke;
mod layers;
mod mappings;
mod model;
mod perms;
mod poll;
mod req;
mod resolve;
mod role;
mod rt;
mod settings;
mod signing;
mod stream;
mod urls;
mod web;
mod zipread;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use aws_model_lambda::Operation;
use aws_model_lambda::meta::META;
use aws_model_lambda::tables::TABLES;
use aws_peers::Peers;
use aws_schema::json::Violation;
use aws_schema::patterns::Patterns;
use aws_wire::ApiError;
use aws_wire::handler::reply_json;
use aws_wire::rest::{RestReply, RestService, Upload};
use aws_wire::service::{BoxFuture, Call, Reply, Service, Web};
use doze_runtime::{Clock, Ctx, Keyed, Tasks};
use serde_json::Value;

use crate::core::{Answer, Ask, Core, Staged};
use crate::req::Req;
use crate::rt::Runtime;

pub use crate::rt::RuntimeConfig;
pub use crate::rt::spec::Interpreters;

/// The region whose files are kept in the service's directory itself.
const HOME_REGION: &str = "us-east-1";

/// What Lambda is set up with.
#[derive(Clone, Debug, Default)]
pub struct Config {
    /// How the processes of functions are run.
    pub runtime: RuntimeConfig,
    /// Keeps what functions print out of doze-aws's own log (it is always in CloudWatch Logs).
    pub quiet: bool,
}

/// The streams and queues that tell a poller they have something, which Lambda is handed because the bus does not carry them.
#[derive(Debug, Clone)]
pub struct Sources {
    /// SQS.
    pub sqs: Arc<aws_sqs::Sqs>,
    /// Kinesis: [`aws_kinesis::Kinesis::appended`] says records arrived.
    pub kinesis: Arc<aws_kinesis::Kinesis>,
    /// DynamoDB: [`aws_dynamodb::Dynamodb::stream_arrived`] says a stream got records.
    pub dynamodb: Arc<aws_dynamodb::Dynamodb>,
}

/// The Lambda service. Nothing runs until the first request, except that a region left with enabled mappings is resumed by
/// [`Lambda::resume`].
#[derive(Debug, Clone)]
pub struct Lambda {
    inner: Arc<Inner>,
}

#[derive(Debug)]
pub(crate) struct Inner {
    actors: Keyed<Core>,
    dir: PathBuf,
    pub(crate) tasks: Tasks,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) rt: Runtime,
    pub(crate) logger: rt::logs::Logger,
    pub(crate) roles: role::Sessions,
    pub(crate) config: Config,
    pub(crate) sources: OnceLock<Sources>,
    /// The address the gateway listens on, as functions reach it.
    endpoint: Mutex<String>,
    account: Mutex<String>,
    pub(crate) pollers: poll::Pollers,
    patterns: Patterns,
}

impl Lambda {
    /// Registers Lambda. `ctx` says where its files live and how it reads the time.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        Self::with_config(ctx, Config::default())
    }

    /// Registers Lambda with settings.
    #[must_use]
    pub fn with_config(ctx: Arc<Ctx>, config: Config) -> Self {
        let peers = ctx
            .ext
            .as_ref()
            .and_then(|e| Arc::clone(e).downcast::<Peers>().ok());
        let inner = Arc::new(Inner {
            actors: Keyed::new("lambda", Arc::clone(&ctx), HOME_REGION),
            dir: ctx.dir.clone(),
            tasks: ctx.tasks.clone(),
            clock: Arc::clone(&ctx.clock),
            peers,
            rt: Runtime::new(
                ctx.tasks.clone(),
                Arc::clone(&ctx.clock),
                config.runtime.clone(),
                ctx.dir.clone(),
            ),
            logger: rt::logs::Logger::default(),
            roles: role::Sessions::default(),
            config,
            sources: OnceLock::new(),
            endpoint: Mutex::new("http://127.0.0.1:4566".to_owned()),
            account: Mutex::new("000000000000".to_owned()),
            pollers: poll::Pollers::default(),
            patterns: Patterns::new(&TABLES),
        });
        Self { inner }
    }

    /// Hands Lambda the services that wake its pollers. Without them event source mappings are stored and nothing polls.
    pub fn connect(&self, sources: Sources) {
        // WHY: set once at startup; a second call changes nothing, which is what a caller that calls twice means.
        drop(self.inner.sources.set(sources));
    }

    /// Tells functions where the other services are (`http://127.0.0.1:4566`), for `AWS_ENDPOINT_URL`.
    pub fn set_endpoint(&self, endpoint: &str) {
        endpoint.clone_into(
            &mut self
                .inner
                .endpoint
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
    }

    /// Starts the pollers of the mappings that were enabled when the process last stopped.
    pub async fn resume(&self) {
        poll::resume(&self.inner).await;
    }
}

/// The region a service directory is for: `regions/<region>` below the service's own, or the home region.
pub(crate) fn region_of(dir: &Path) -> String {
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(HOME_REGION);
    let in_regions = dir
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|p| p == "regions");
    if in_regions {
        name.to_owned()
    } else {
        HOME_REGION.to_owned()
    }
}

impl Inner {
    /// Asks the actor of `region`.
    pub(crate) async fn ask(&self, region: &str, ask: Ask) -> Result<Answer, ApiError> {
        let actor = self
            .actors
            .actor(region)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        actor
            .call(ask)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))
    }

    pub(crate) fn endpoint_for(&self, _caller: &aws_wire::service::Caller) -> String {
        self.endpoint
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The directory the files of `region` are in.
    pub(crate) fn region_dir(&self, region: &str) -> PathBuf {
        if region == HOME_REGION {
            self.dir.clone()
        } else {
            self.dir.join("regions").join(region)
        }
    }

    /// Learns where the gateway is from a request that reached it (a client's `Host` is the address functions can use too).
    fn note_endpoint(&self, call: &Call) {
        if call.caller.endpoint.starts_with("http") {
            call.caller
                .endpoint
                .clone_into(&mut self.endpoint.lock().unwrap_or_else(PoisonError::into_inner));
        }
        call.caller
            .account_id
            .clone_into(&mut self.account.lock().unwrap_or_else(PoisonError::into_inner));
    }

    /// The account the requests seen so far were for (a function URL request carries none).
    pub(crate) fn account_of(&self, _region: &str) -> String {
        self.account
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Runs a call: the operations that run a function are done beside the actor, the others in it, with the code or the role they
    /// need looked at first.
    pub(crate) async fn run(self: &Arc<Self>, call: Call) -> Result<RestReply, ApiError> {
        self.note_endpoint(&call);
        match Operation::from_index(call.op) {
            Some(Operation::Invoke) => self.invoke(call).await,
            Some(Operation::InvokeWithResponseStream) => self.invoke_stream(call).await,
            Some(op) => self.manage(op, call).await,
            None => Err(errors::refused("This operation")),
        }
    }

    async fn manage(self: &Arc<Self>, op: Operation, call: Call) -> Result<RestReply, ApiError> {
        let region = call.caller.region.clone();
        let r = Req::new(&call);
        let staged = self.stage(op, &r).await?;
        let answer = self
            .ask(
                &region,
                Ask::Call {
                    call: Box::new(call),
                    staged,
                },
            )
            .await?;
        let Answer::Reply {
            reply,
            durable,
            retire,
        } = answer
        else {
            return Err(ApiError::internal(
                "Lambda answered a call with something else",
            ));
        };
        if let Some(saved) = durable {
            saved
                .await
                .map_err(|e| ApiError::internal(format!("could not save: {e}")))?;
        }
        for name in retire {
            self.rt.retire(&format!("{region}/{name}"));
        }
        if matches!(
            op,
            Operation::CreateEventSourceMapping
                | Operation::UpdateEventSourceMapping
                | Operation::DeleteEventSourceMapping
        ) {
            poll::sync(self, &region).await;
        }
        let out = reply?;
        Ok(RestReply {
            json: out.json,
            status: out.status,
            body: None,
        })
    }

    /// What an operation needs read before its state is touched: the code it brings, and the role it names.
    async fn stage(&self, op: Operation, r: &Req) -> Result<Staged, ApiError> {
        let mut staged = Staged::default();
        let base = self.region_dir(r.region());
        let spec = match op {
            Operation::CreateFunction => r.value("Code"),
            Operation::PublishLayerVersion => r.value("Content"),
            Operation::UpdateFunctionCode if r.flag("DryRun") != Some(true) => Some(&r.body),
            _ => None,
        };
        if let Some(spec) = spec {
            staged.code = Some(code::stage(&base, self.peers.as_deref(), &r.caller, spec).await?);
        }
        if matches!(
            op,
            Operation::CreateFunction | Operation::UpdateFunctionConfiguration
        ) && let Some(role) = r.opt("Role")
        {
            staged.role = role::check(self.peers.as_deref(), &r.caller, role).await;
        }
        Ok(staged)
    }
}

impl Service for Lambda {
    fn meta(&self) -> &'static aws_schema::meta::ServiceMeta {
        &META
    }

    fn tables(&self) -> &'static aws_schema::tables::Tables {
        &TABLES
    }

    fn serves(&self, op: u32) -> bool {
        Operation::from_index(op).is_some_and(core::built)
    }

    /// A call from a peer (S3, SNS, EventBridge, CloudWatch invoking a function; Step Functions): the same operations, answered as
    /// the model's output.
    fn call(&self, call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(async move {
            let op = call.op;
            let reply = self.inner.run(call).await?;
            reply_json(&TABLES, &self.inner.patterns, op, &reply.json)
        })
    }

    fn rest(&self) -> Option<&dyn RestService> {
        Some(self)
    }

    fn web(&self) -> Option<&dyn Web> {
        Some(self)
    }

    /// Lambda's front end words a request that breaks the model's rules as a `ValidationException`; a runtime that is not in the
    /// model's list also says it may be an ARN (recorded 2026-11-09).
    fn invalid(&self, violation: &Violation) -> ApiError {
        let mut e = aws_wire::service::default_invalid(&TABLES, violation);
        if let Violation::Enum { member, value } = violation
            && TABLES.member(*member).is_some_and(|m| m.name == "Runtime")
        {
            e.message = format!(
                "1 validation error detected: Value '{}' at 'runtime' failed to satisfy constraint: Member must satisfy enum value set: [{}] or be a valid ARN",
                value.as_deref().unwrap_or("null"),
                config::RUNTIMES
            );
        }
        if matches!(violation, Violation::Syntax { .. } | Violation::TooDeep) {
            return errors::invalid_content("Could not parse request body into json.");
        }
        e
    }
}

impl RestService for Lambda {
    fn call_rest(
        &self,
        call: Call,
        _upload: Option<Box<dyn Upload>>,
    ) -> BoxFuture<'_, Result<RestReply, ApiError>> {
        Box::pin(async move { self.inner.run(call).await })
    }
}

impl Web for Lambda {
    fn owns(&self, host: &str, path: &str) -> bool {
        web::owns(host, path)
    }

    fn serve(
        &self,
        request: aws_wire::service::WebRequest,
    ) -> BoxFuture<'_, aws_wire::service::WebReply> {
        Box::pin(web::serve(Arc::clone(&self.inner), request))
    }
}

/// The JSON `Value` of a number as the integer it is, for the sake of callers that compare.
pub(crate) fn int(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))
}

/// Sets a member of a JSON object (a value that is not one is left as it is).
pub(crate) fn set(v: &mut Value, key: &str, value: Value) {
    if let Some(o) = v.as_object_mut() {
        o.insert(key.to_owned(), value);
    }
}

#[cfg(test)]
mod tests;
