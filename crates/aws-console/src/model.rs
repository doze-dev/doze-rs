//! The contract: what the JSON API answers with. Each type derives `serde::Serialize` and, under test, `ts_rs::TS`; the test in
//! `api/types.rs` writes one TypeScript file per type into `console/apps/aws/src/api/types/` and fails when one is stale, so the
//! page layer cannot drift from the server.
//!
//! How to add one: the type here, a line in `api/types.rs`, then `cargo test -p aws-console api::types` (it writes the file and
//! fails once; commit the file).

use serde::Serialize;

use crate::facts::Facts;
use crate::ring::{Call, Flow};

mod deep;
mod extras;
mod resources;
mod spaces;
mod wiring;
pub use deep::*;
pub use extras::*;
pub use resources::*;
pub use spaces::*;
pub use wiring::*;

/// A refusal: a stable code a script can match and the words a person reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ApiError {
    /// `FORBIDDEN`, `NOT_FOUND`, `METHOD_NOT_ALLOWED`, `BAD_REQUEST`, `BUSY`.
    pub code: String,
    pub message: String,
}

/// One service of the build, by the name `--services` takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ServiceName {
    /// `stepfunctions`.
    pub name: String,
    /// `Step Functions`.
    pub title: String,
    /// Whether this run serves it (`--services`); one that is not answers every call with how to turn it on.
    pub served: bool,
}

/// `GET /api/v1/about`: this doze-aws.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct About {
    /// `doze-aws`.
    pub name: String,
    pub version: String,
    /// The twelve-digit account every request is for.
    pub account: String,
    /// The default region (`--region`).
    pub region: String,
    /// The default region and every region a call in the Live record was signed for, sorted.
    pub regions: Vec<String>,
    /// The URL AWS SDKs and the AWS CLI reach doze-aws at.
    pub endpoint: String,
    /// The URL of this console.
    pub console: String,
    /// `trust` (any key, signatures not checked) or `verify`.
    pub auth: String,
    /// What IAM does with a request its policies refuse: `off`, `soft` (logged, let through) or `enforce`.
    pub iam: String,
    pub data_dir: String,
    pub pid: u32,
    /// When the process started, in seconds since 1970.
    pub started: u64,
    /// Every service of the build, in catalogue order.
    pub services: Vec<ServiceName>,
}

/// One API of a service and how much of AWS's model of it is built.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ApiCoverage {
    /// The SDK id (`DynamoDB Streams`).
    pub sdk_id: String,
    /// The name in a credential scope (`dynamodb`).
    pub signing_name: String,
    /// Operations this build answers.
    pub built: u32,
    /// Operations in AWS's model.
    pub total: u32,
}

/// One service and how much of it is built.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ServiceRow {
    pub name: String,
    pub title: String,
    /// What it is for, as the Resources index groups it (`Messaging and events`).
    pub group: String,
    pub served: bool,
    /// Operations this build answers, over every API of the service.
    pub built: u32,
    /// Operations in AWS's models of them.
    pub total: u32,
    /// Each API (most services have one; DynamoDB has its Streams, API Gateway has three).
    pub apis: Vec<ApiCoverage>,
}

/// `GET /api/v1/services`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ServicesResponse {
    pub services: Vec<ServiceRow>,
}

/// How a call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    /// Answered as asked (a status under 400).
    Ok,
    /// Any other error answer, 4xx or 5xx, the caller's or the server's, a validation error included: the status says which.
    Error,
    /// IAM refused it (an `AccessDenied...` error, under `--iam enforce`): the caller may not do this.
    Denied,
    /// doze-aws does not do this: the operation is not built (501 `NotImplemented`), or the service is not served in this run
    /// (`OptInRequired`, the answer to a service left out of `--services`).
    NotBuilt,
}

impl Outcome {
    /// The outcome of an answer with `status` and error `code`.
    #[must_use]
    pub fn of(status: u16, code: &str) -> Self {
        if status < 400 {
            Self::Ok
        } else if code.starts_with("AccessDenied") {
            Self::Denied
        } else if (status == 501 && code == "NotImplemented") || code == "OptInRequired" {
            Self::NotBuilt
        } else {
            Self::Error
        }
    }
}

/// One finished AWS API call, as the gateway saw it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CallRecord {
    /// Its number in this process: 1 for the first call, no gaps.
    pub seq: u64,
    /// When it was answered, in milliseconds since 1970.
    pub at_ms: u64,
    /// The service by its `--services` name (`stepfunctions`); the signing name for one the catalogue does not hold; `null` when
    /// the request named no service of this build.
    pub service: Option<String>,
    /// The API's SDK id (`SQS`, `DynamoDB Streams`: a service can have more than one API); `null` with `service`.
    pub api: Option<String>,
    /// The operation (`SendMessage`); `null` when the request was refused before one was found.
    pub action: Option<String>,
    /// What was acted on, cut at 256 bytes: an S3 path (`bucket/key`), or the value of the input member `member` names (a queue
    /// URL, a table name, an ARN), as the caller wrote it. `null` when the call named nothing the gateway knows to look for.
    pub resource: Option<String>,
    /// The input member `resource` is the value of (`QueueUrl`, `TableName`, `stateMachineArn`); `null` for an S3 path,
    /// which is read from the address, and with `resource`.
    pub member: Option<String>,
    /// The access key id the request was signed with; `null` for one that carried none. For a call between services, the key of
    /// the caller it was made for.
    pub caller: Option<String>,
    /// The region it was signed for.
    pub region: Option<String>,
    /// The HTTP status answered.
    pub status: u16,
    /// The error code answered (`QueueDoesNotExist`); `null` for a success.
    pub error_code: Option<String>,
    pub outcome: Outcome,
    /// `--iam soft` found the caller's policies refuse this call and let it through: under `--iam enforce` it would be `denied`.
    pub would_be_denied: bool,
    /// From the request's head being read to its answer being made, in microseconds.
    pub duration_us: u32,
    /// The id the answer carried (`x-amzn-RequestId`); empty for a call between services, which has no answer on the wire.
    pub request_id: String,
    /// `null` for a request from the wire. Set for a call one service made to another on doze-aws's internal bus (SNS delivering
    /// to SQS, a rule invoking a function): no socket, no signature, and no request id of its own.
    pub via: Option<CallVia>,
    /// The client that sent it, as its `User-Agent` names it (`boto3 1.34.69`, `AWS CLI 2.15.30`, `JS v3 3.540.0`); `null` for a
    /// call between services and a request that named none.
    pub sdk: Option<String>,
}

/// What made a call between services.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CallVia {
    /// The service that made it, by its `--services` name (`sns`); `null` for a call made as the caller itself (a Step
    /// Functions task, as its role).
    pub by: Option<String>,
    /// The request id of the call from the wire being answered when it was made (SNS delivering inside `Publish`): that call's
    /// `request_id`. It is recorded after this one, since it is answered after. `null` when no request was being answered.
    pub request_id: Option<String>,
    /// The rule, subscription, mapping or schedule that made it, when the service said; `null` otherwise.
    pub name: Option<String>,
}

impl CallRecord {
    /// The record of `call`, its services named as `facts` names them.
    #[must_use]
    pub fn new(call: &Call, facts: &Facts) -> Self {
        let some = |s: &str| (!s.is_empty()).then(|| s.to_owned());
        Self {
            seq: call.seq,
            at_ms: call.at_ms,
            service: some(facts.service_named(call.signing_name)),
            api: some(call.sdk_id),
            action: some(call.action),
            resource: some(&call.resource),
            member: some(call.member).filter(|_| !call.resource.is_empty()),
            caller: some(&call.caller),
            region: some(&call.region),
            status: call.status,
            error_code: some(&call.code),
            outcome: Outcome::of(call.status, &call.code),
            would_be_denied: call.would_be_denied,
            duration_us: call.took_us,
            request_id: call.request_id.to_string(),
            via: call.via.as_ref().map(|v| CallVia {
                by: some(facts.catalogue_name(&v.by).unwrap_or(&v.by)),
                request_id: some(&v.request),
                name: some(&v.name),
            }),
            sdk: some(&call.sdk),
        }
    }
}

/// The calls of one caller to one service since doze-aws started: a line of the Live map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FlowRow {
    /// The access key; for calls between services (`internal`), the service that made them, by its `--services` name, or the key
    /// of the caller they were made for when no service said. Empty for requests that carried no key.
    pub caller: String,
    /// Calls between services, not requests from the wire.
    pub internal: bool,
    /// Calls the console made, or that a service made for it (caller `console`): Live hides them unless asked.
    pub console: bool,
    /// As a call's `service`; `null` for requests that named no service of this build.
    pub service: Option<String>,
    /// As a call's `api`.
    pub api: Option<String>,
    pub calls: u64,
    pub ok: u64,
    pub error: u64,
    pub denied: u64,
    pub not_built: u64,
    pub would_be_denied: u64,
    /// When its first and newest calls were answered, in milliseconds since 1970.
    pub first_ms: u64,
    pub last_ms: u64,
    /// The number of its newest call.
    pub last_seq: u64,
}

impl FlowRow {
    /// The row of `flow`.
    #[must_use]
    pub fn new(flow: &Flow, facts: &Facts) -> Self {
        let some = |s: &str| (!s.is_empty()).then(|| s.to_owned());
        let [ok, error, denied, not_built] = flow.outcomes;
        Self {
            caller: if flow.internal {
                facts
                    .catalogue_name(&flow.caller)
                    .unwrap_or(&flow.caller)
                    .to_owned()
            } else {
                flow.caller.to_string()
            },
            internal: flow.internal,
            console: flow.console,
            service: some(facts.service_named(flow.signing_name)),
            api: some(flow.sdk_id),
            calls: flow.calls,
            ok,
            error,
            denied,
            not_built,
            would_be_denied: flow.would_be_denied,
            first_ms: flow.first_ms,
            last_ms: flow.last_ms,
            last_seq: flow.last_seq,
        }
    }
}

/// `GET /api/v1/flows`: who called which service, counted as calls are answered (not only the calls the record still holds).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FlowsResponse {
    /// The most recently called first.
    pub flows: Vec<FlowRow>,
    /// The most flows kept: past it, the one quiet the longest is let go.
    pub capacity: u32,
    /// Flows let go since doze-aws started.
    pub dropped: u64,
    /// The number of the newest call counted: a call on the stream numbered above it is not in these counts yet.
    pub through_seq: u64,
}

/// `GET /api/v1/calls/<seq>/copy`: one call as commands to run again. The record keeps no request body, so a command carries
/// only what the record has: the operation, the region, the endpoint, and the one input member that named the resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CopyCommands {
    pub seq: u64,
    /// An AWS CLI command (`aws --endpoint-url … sqs send-message --queue-url …`); `null` when the call named no operation.
    pub cli: Option<String>,
    /// A curl command, signed by curl itself (`--aws-sigv4`) with the keys in the environment; `null` for a protocol it cannot
    /// be written for.
    pub curl: Option<String>,
    /// The input members filled in (`QueueUrl`): every other parameter the call carried is not known.
    pub filled: Vec<String>,
}

/// How full the Live record is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RingFill {
    /// The most calls it holds.
    pub capacity: u32,
    /// Calls it holds now.
    pub held: u32,
    /// Calls recorded since the process started.
    pub recorded: u64,
    /// The memory the held calls take, in bytes.
    pub bytes: u64,
}

/// `GET /api/v1/calls`, and the `calls` event of `GET /api/v1/tail/live`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CallsPage {
    /// Oldest first.
    pub calls: Vec<CallRecord>,
    /// What to pass as `since` to be given what comes after these.
    pub cursor: String,
    /// The `since` asked with was of another run of doze-aws (or ahead of this one): drop what you hold, these are the newest.
    pub reset: bool,
    /// Calls after `since` that the record no longer holds (it keeps the newest `ring.capacity`).
    pub missed: u64,
    /// More calls after `cursor` are held: ask again.
    pub more: bool,
    pub ring: RingFill,
}

/// The `ready` event of `GET /api/v1/tail/live`: the stream is open and will send what comes after `cursor`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TailReady {
    pub cursor: String,
    /// The cursor asked with was of another run: drop what you hold; the stream sends this run's calls from the first held.
    pub reset: bool,
}
