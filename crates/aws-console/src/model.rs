//! The contract: what the JSON API answers with. Each type derives `serde::Serialize` and, under test, `ts_rs::TS`; the test in
//! `api/types.rs` writes one TypeScript file per type into `console/apps/aws/src/api/types/` and fails when one is stale, so the
//! page layer cannot drift from the server.
//!
//! How to add one: the type here, a line in `api/types.rs`, then `cargo test -p aws-console api::types` (it writes the file and
//! fails once; commit the file).

use serde::Serialize;

use crate::ring::Call;

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
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Answered as asked (a status under 400).
    Ok,
    /// IAM said no (an `AccessDenied...` error): the caller may not do this.
    Denied,
    /// doze-aws itself does not do this: the operation is not built (501 `NotImplemented`), or the service was left out of
    /// `--services` (`OptInRequired`).
    Refused,
    /// Any other error, the caller's or the server's: the status says which.
    Error,
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
            Self::Refused
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
    /// What was acted on, when the gateway itself knows: an S3 path (`bucket/key`, cut at 256 bytes). `null` for every other
    /// service until the services name their resources.
    pub resource: Option<String>,
    /// The access key id the request was signed with; `null` for one that carried none.
    pub caller: Option<String>,
    /// The region it was signed for.
    pub region: Option<String>,
    /// The HTTP status answered.
    pub status: u16,
    /// The error code answered (`QueueDoesNotExist`); `null` for a success.
    pub error_code: Option<String>,
    pub outcome: Outcome,
    /// From the request's head being read to its answer being made, in microseconds.
    pub duration_us: u32,
    /// The id the answer carried (`x-amzn-RequestId`).
    pub request_id: String,
}

impl CallRecord {
    /// The record of `call`, its service named `service`.
    #[must_use]
    pub fn new(call: &Call, service: &str) -> Self {
        let some = |s: &str| (!s.is_empty()).then(|| s.to_owned());
        Self {
            seq: call.seq,
            at_ms: call.at_ms,
            service: some(service),
            api: some(call.sdk_id),
            action: some(call.action),
            resource: some(&call.resource),
            caller: some(&call.caller),
            region: some(&call.region),
            status: call.status,
            error_code: some(&call.code),
            outcome: Outcome::of(call.status, &call.code),
            duration_us: call.took_us,
            request_id: call.request_id.to_string(),
        }
    }
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
