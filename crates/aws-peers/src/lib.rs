//! The bus between services. A service that needs another (SSM sealing a `SecureString` with KMS, SNS delivering to SQS) calls the
//! other's operation by name, with a JSON body, as itself acting for the caller. The call goes through the other service's own
//! model: the body is decoded and checked as a request from the wire would be, the service answers as it would, and the answer
//! is read back as JSON. There is no socket, no signature and no URL.
//!
//! A service never calls one that calls it back while it waits (that would wait forever); the order in use is SSM and Secrets
//! Manager to KMS, SSM to Secrets Manager, and, later, S3, SNS, EventBridge and Lambda to SQS, SNS and Lambda.

mod authz;
pub mod envelope;
mod metrics;
pub mod naming;
pub mod seen;

pub use authz::{Authorizer, Mode, Need};
pub use metrics::{Made, Producer};
pub use seen::{BusCall, CallWatcher, Cause};

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use aws_schema::arena::{Arena, NIL};
use aws_schema::enc::encode;
use aws_schema::json::decode;
use aws_schema::patterns::Patterns;
use aws_wire::service::{Call, Caller, Service};
use aws_wire::{ApiError, RequestId};
use serde_json::Value;

/// A call ready to go: its target, the decoded call, and its watch when the bus is watched.
type Prepared = (Arc<dyn Service>, Call, Option<seen::Pending>);

struct Entry {
    service: Arc<dyn Service>,
    patterns: Patterns,
}

/// The services that can be called, by the name they are signed for (`kms`, `sqs`, `secretsmanager`).
#[derive(Default)]
pub struct Peers {
    entries: RwLock<HashMap<&'static str, Entry>>,
    authorizer: RwLock<Option<Arc<dyn Authorizer>>>,
    mode: authz::ModeCell,
    would_deny: RwLock<Option<authz::WouldDeny>>,
    metrics: RwLock<metrics::Registry>,
    watcher: RwLock<Option<Arc<dyn CallWatcher>>>,
}

impl std::fmt::Debug for Peers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Peers")
    }
}

impl Peers {
    /// An empty bus.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Tells `watcher` about every call on the bus from now on (the console's Live record). Unset, a call pays one read lock.
    pub fn watch_calls(&self, watcher: Arc<dyn CallWatcher>) {
        if let Ok(mut slot) = self.watcher.write() {
            *slot = Some(watcher);
        }
    }

    /// Whether anyone watches the bus's calls: a service names the cause of its work ([`seen::named`]) only then.
    #[must_use]
    pub fn calls_watched(&self) -> bool {
        self.watcher.read().is_ok_and(|w| w.is_some())
    }

    /// Makes `service` callable by its signing name.
    pub fn register(&self, service: Arc<dyn Service>) {
        let name = service.meta().signing_name;
        self.register_as(name, service);
    }

    /// Makes `service` callable as `name`, for a service whose signing name another already has (API Gateway's HTTP APIs sign as
    /// `apigateway` like its REST APIs, and are called `apigatewayv2`).
    pub fn register_as(&self, name: &'static str, service: Arc<dyn Service>) {
        let patterns = Patterns::new(service.tables());
        if let Ok(mut entries) = self.entries.write() {
            entries.insert(name, Entry { service, patterns });
        }
    }

    /// Every service that can be called, by its name on the bus, in name order (the console's Lab lists them).
    #[must_use]
    pub fn targets(&self) -> Vec<(&'static str, Arc<dyn Service>)> {
        let Ok(entries) = self.entries.read() else {
            return Vec::new();
        };
        let mut all: Vec<(&'static str, Arc<dyn Service>)> = entries
            .iter()
            .map(|(name, e)| (*name, Arc::clone(&e.service)))
            .collect();
        all.sort_by_key(|(name, _)| *name);
        all
    }

    /// The model tables of service `target`, when it is registered (a caller that builds bodies from loosely typed input, such as
    /// a CloudFormation template, reads them to coerce its values).
    #[must_use]
    pub fn tables(&self, target: &str) -> Option<&'static aws_schema::tables::Tables> {
        let entries = self.entries.read().ok()?;
        entries.get(target).map(|e| e.service.tables())
    }

    /// Whether service `target` speaks a REST protocol (S3, Lambda, Scheduler): a caller calls it with [`Peers::call_rest_as`].
    #[must_use]
    pub fn speaks_rest(&self, target: &str) -> bool {
        self.entries.read().is_ok_and(|entries| {
            entries
                .get(target)
                .is_some_and(|e| e.service.rest().is_some())
        })
    }

    /// Calls operation `operation` of service `target` with `body`, as service `acting` on behalf of `caller`. The answer is the
    /// operation's output as JSON (`{}` for an operation with none).
    ///
    /// # Errors
    /// The target's own error; or an internal error if there is no such service or operation, or the answer cannot be read.
    pub async fn call(
        &self,
        acting: &str,
        caller: &Caller,
        target: &str,
        operation: &str,
        body: &Value,
    ) -> Result<Value, ApiError> {
        self.call_inner(Some(acting), caller, target, operation, body)
            .await
    }

    /// Like [`Peers::call`], made as `caller` itself rather than as a service acting for it: the target asks the authorizer about
    /// `caller` as it would about a request from the wire. A service that runs work as a role (Step Functions' tasks) calls this
    /// with the role's session.
    ///
    /// # Errors
    /// As [`Peers::call`], and the authorizer's refusal.
    pub async fn call_as(
        &self,
        caller: &Caller,
        target: &str,
        operation: &str,
        body: &Value,
    ) -> Result<Value, ApiError> {
        self.call_inner(None, caller, target, operation, body).await
    }

    async fn call_inner(
        &self,
        acting: Option<&str>,
        caller: &Caller,
        target: &str,
        operation: &str,
        body: &Value,
    ) -> Result<Value, ApiError> {
        let (service, call, seen) = self.prepare(acting, caller, target, operation, body)?;
        let output = told(seen, service.call(call).await)?;
        let tables = service.tables();
        let shape = tables
            .find_op(operation)
            .and_then(|i| tables.op(i))
            .and_then(|o| o.output);
        let Some(shape) = shape.filter(|_| output.root != NIL) else {
            return Ok(Value::Object(serde_json::Map::new()));
        };
        let mut bytes = Vec::new();
        encode(tables, shape, &output.arena, "", output.root, &mut bytes)
            .map_err(|_| ApiError::internal("a peer's answer could not be written"))?;
        serde_json::from_slice(&bytes)
            .map_err(|_| ApiError::internal("a peer's answer could not be read"))
    }

    /// Like [`Peers::call`], for a target that speaks a REST protocol (S3): the answer keeps its streamed body, which the caller reads.
    /// There is no request body stream; an operation that needs one cannot be called this way.
    ///
    /// # Errors
    /// The target's own error; or an internal error if there is no such service or operation, or it has no REST side.
    pub async fn call_rest(
        &self,
        acting: &str,
        caller: &Caller,
        target: &str,
        operation: &str,
        body: &Value,
    ) -> Result<aws_wire::rest::RestReply, ApiError> {
        let (service, call, seen) = self.prepare(Some(acting), caller, target, operation, body)?;
        let rest = service
            .rest()
            .ok_or_else(|| ApiError::internal(format!("{target} has no REST side")))?;
        told(seen, rest.call_rest(call, None).await)
    }

    /// Like [`Peers::call_rest`], made as `caller` itself (see [`Peers::call_as`]), with an optional request body stream.
    ///
    /// # Errors
    /// As [`Peers::call_rest`], and the authorizer's refusal.
    pub async fn call_rest_as(
        &self,
        caller: &Caller,
        target: &str,
        operation: &str,
        body: &Value,
        upload: Option<Box<dyn aws_wire::rest::Upload>>,
    ) -> Result<aws_wire::rest::RestReply, ApiError> {
        let (service, call, seen) = self.prepare(None, caller, target, operation, body)?;
        let rest = service
            .rest()
            .ok_or_else(|| ApiError::internal(format!("{target} has no REST side")))?;
        told(seen, rest.call_rest(call, upload).await)
    }

    /// Like [`Peers::call_as`], for an operation whose answer is an event stream (CloudWatch Logs' `StartLiveTail`): the framed
    /// messages (`aws_wire::eventstream`), read by the caller as the target writes them.
    ///
    /// # Errors
    /// As [`Peers::call_as`]; an internal error when the operation does not stream.
    pub async fn call_stream_as(
        &self,
        caller: &Caller,
        target: &str,
        operation: &str,
        body: &Value,
    ) -> Result<aws_wire::rest::Download, ApiError> {
        let (service, call, seen) = self.prepare(None, caller, target, operation, body)?;
        if !service.streams(call.op) {
            return told(
                seen,
                Err(ApiError::internal(format!(
                    "{operation} of {target} does not stream"
                ))),
            );
        }
        told(seen, service.call_stream(call).await)
    }

    /// The model of a service on the bus, when it runs.
    #[must_use]
    pub fn tables_of(&self, target: &str) -> Option<&'static aws_schema::tables::Tables> {
        let entries = self.entries.read().ok()?;
        entries.get(target).map(|e| e.service.tables())
    }

    /// Like [`Peers::call_rest`], with a request body held in memory (an object CloudFormation puts in S3 for a bucket deployment).
    ///
    /// # Errors
    /// As [`Peers::call_rest`].
    pub async fn call_rest_with_body(
        &self,
        acting: &str,
        caller: &Caller,
        (target, operation): (&str, &str),
        body: &Value,
        payload: bytes::Bytes,
    ) -> Result<aws_wire::rest::RestReply, ApiError> {
        let (service, call, seen) = self.prepare(Some(acting), caller, target, operation, body)?;
        let rest = service
            .rest()
            .ok_or_else(|| ApiError::internal(format!("{target} has no REST side")))?;
        told(
            seen,
            rest.call_rest(call, Some(Box::new(Whole(Some(payload)))))
                .await,
        )
    }

    /// The target service and the decoded call.
    fn prepare(
        &self,
        acting: Option<&str>,
        caller: &Caller,
        target: &str,
        operation: &str,
        body: &Value,
    ) -> Result<Prepared, ApiError> {
        let entries = self
            .entries
            .read()
            .map_err(|_| ApiError::internal("the peer bus is unavailable"))?;
        let entry = entries.get(target).ok_or_else(|| {
            ApiError::internal(format!("no service {target} is running to be called"))
        })?;
        let tables = entry.service.tables();
        let op = tables
            .find_op(operation)
            .ok_or_else(|| ApiError::internal(format!("{target} has no operation {operation}")))?;
        if !entry.service.serves(op) {
            return Err(ApiError::new(
                501,
                "NotImplemented",
                format!("{operation} is not built in doze-aws."),
            ));
        }
        let shape = tables.op(op).and_then(|o| o.input);
        let text = body.to_string();
        let mut arena = Arena::new();
        let root = match shape {
            Some(shape) => decode(tables, &entry.patterns, shape, &text, &mut arena)
                .map_err(|v| entry.service.invalid(&v))?,
            None => NIL,
        };
        let mut caller = caller.clone();
        if let Some(acting) = acting {
            caller.service = Some(acting.to_owned());
        }
        let call = Call {
            op,
            id: RequestId::new(),
            caller,
            body: text,
            arena,
            root,
        };
        let watcher = self.watcher.read().ok().and_then(|w| w.clone());
        let seen = watcher.and_then(|w| seen::Pending::begin(w, &*entry.service, &call));
        Ok((Arc::clone(&entry.service), call, seen))
    }
}

/// `answer`, told to the watcher of the call if it has one.
fn told<T>(seen: Option<seen::Pending>, answer: Result<T, ApiError>) -> Result<T, ApiError> {
    if let Some(seen) = seen {
        seen.end(answer.as_ref().err());
    }
    answer
}

/// A request body held whole, handed to a REST service as one piece.
struct Whole(Option<bytes::Bytes>);

impl aws_wire::rest::Upload for Whole {
    fn next(&mut self) -> aws_wire::service::BoxFuture<'_, Result<Option<bytes::Bytes>, ApiError>> {
        let piece = self.0.take();
        Box::pin(async move { Ok(piece) })
    }

    fn trailers(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    fn length(&self) -> Option<u64> {
        self.0.as_ref().map(|b| b.len() as u64)
    }
}

#[cfg(test)]
mod tests;
