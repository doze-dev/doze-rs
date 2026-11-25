//! Fault rules at the edge of the gateway, on purpose, for a while (the console's Lab: `docs/port/aws/lab.md`).
//!
//! Each protocol path asks the [`Faults`] installed in [`crate::Config::faults`] about a request **after it is decoded** (so the
//! service, the operation and what it names are known) and **before the service is asked**. The answer is to let it through, to
//! make it wait, to answer an error in its place, or to close the connection:
//!
//! - **Slow**: the request waits (a stop of the server does not wait), then goes on as if nothing had happened.
//! - **Fail**: the path's own error renderer words the error, so it has the shape of the protocol (a JSON body, Query XML, CBOR,
//!   S3's XML, a REST JSON header) and its code is in every place an SDK reads one. The service is never asked: a fault never
//!   reaches storage, and a failed write writes nothing.
//! - **Drop**: the connection is closed with no answer.
//!
//! With no [`Faults`] installed (the default, and `--lab=false`) a request costs one `Option` test. With one installed and no rule
//! switched on, one call to [`Faults::armed`]: a relaxed atomic load in the table.
//!
//! Not judged: `/_doze/health`, the local hooks under `/_doze/`, function URLs and API Gateway stages (they are not AWS API calls),
//! and anything that does not come through the gateway: the console's own calls, and calls between services over the bus.

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

mod apply;

pub(crate) use apply::{Dropped, Step, apply, is_dropped, marked_dropped};

/// What a rule is shown about a request: the facts the pipeline noted on the way, borrowed.
#[derive(Debug, Clone, Copy)]
pub struct Ask<'a> {
    /// The service's name in a credential scope (`sqs`, `states`).
    pub signing_name: &'static str,
    /// The service's SDK id (`SQS`, `DynamoDB Streams`): two services can share a signing name.
    pub sdk_id: &'static str,
    /// The operation (`SendMessage`).
    pub operation: &'a str,
    /// What the call names (a queue URL, a table, an ARN, an S3 `bucket/key`); empty when it names nothing.
    pub resource: &'a str,
    /// The access key id the request was signed with.
    pub access_key_id: &'a str,
    /// The region it was signed for.
    pub region: &'a str,
    /// The request's `User-Agent`, as sent.
    pub user_agent: &'a str,
}

/// Which rule touched a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// The rule's number.
    pub rule: u32,
    /// Its name when it fired.
    pub name: Arc<str>,
}

/// An error to answer in place of the service's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The error's shape in the service's model, when the model carries it: the wire code, status and fault then come from the model
    /// (a Query service words its code its own way). Otherwise `status` and `code` are the wire's.
    pub shape: Option<&'static str>,
    /// The HTTP status, when the shape is not in the model.
    pub status: u16,
    /// The code a client reads, when the shape is not in the model.
    pub code: Cow<'static, str>,
    /// The words of the error.
    pub message: String,
    /// The rule.
    pub tag: Tag,
}

/// What to do with a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Serve it as usual.
    Pass,
    /// Wait this long, then serve it.
    Slow { delay: Duration, tag: Tag },
    /// Answer with this error and do not ask the service.
    Fail(Box<Failure>),
    /// Close the connection with no answer.
    Drop { tag: Tag },
}

/// Decides, for each request, whether anything is done to it. Called on the request's task, between its decoding and the service:
/// the work must be bounded and must not block.
pub trait Faults: fmt::Debug + Send + Sync + 'static {
    /// Whether any rule could fire now. A relaxed load: `false` lets the request through without anyone looking at it.
    fn armed(&self) -> bool;

    /// What to do with this request.
    fn judge(&self, ask: &Ask<'_>) -> Verdict;
}

/// What a rule did to a request, as the observer is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Done {
    /// The request waited this long before it was served.
    Delayed(Duration),
    /// It was answered with an error and the service never saw it.
    Failed,
    /// The connection was closed with no answer.
    Dropped,
}

/// The rule that touched a finished request ([`crate::Finished::lab`]).
#[derive(Debug, Clone, Copy)]
pub struct LabNote<'a> {
    /// The rule's number.
    pub rule: u32,
    /// Its name.
    pub name: &'a str,
    /// What it did.
    pub done: Done,
}
