//! Calls between services, told to whoever asked ([`crate::Peers::watch_calls`]: the console's Live record), and what caused
//! them.
//!
//! Nothing here runs unless a watcher is set: with none, a bus call reads one lock that holds `None` and goes on.
//!
//! The cause of a bus call is one of two things, both carried by a task-local, so no service passes it by hand:
//! - the request from the wire whose handling made the call (SNS delivering to SQS inside `Publish`): the gateway runs each
//!   request inside [`within_request`] while someone watches, and a bus call made on that task is marked with its request id;
//! - a thing of a service's own that made it on a task of its own (an EventBridge rule, a Lambda event source mapping, a
//!   schedule): the service runs that work inside [`named`], and the call is marked with the name.

use std::future::Future;
use std::time::Duration;

/// What made a bus call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cause {
    /// The id of the request from the wire being answered when the call was made.
    pub request: Option<Box<str>>,
    /// The rule, subscription, mapping or schedule that made it (`rule order-placed`).
    pub name: Option<Box<str>>,
}

tokio::task_local! {
    static CAUSE: Cause;
    static ASKED: ();
}

/// Runs `work` as asked for by a person (the console reading a queue's attributes for its page): a call it makes is told to the
/// watcher even when it is one of the background reads [`is_poll`] would leave out, since a person asked for it.
pub async fn asked<F: Future>(work: F) -> F::Output {
    ASKED.scope((), work).await
}

/// Whether the work running now was asked for by a person ([`asked`]).
#[must_use]
pub fn is_asked() -> bool {
    ASKED.try_with(|()| ()).is_ok()
}

/// Runs `work` as the handling of request `request_id`: a bus call it makes is marked as caused by that request.
pub async fn within_request<F: Future>(request_id: &str, work: F) -> F::Output {
    let name = CAUSE.try_with(|c| c.name.clone()).ok().flatten();
    CAUSE
        .scope(
            Cause {
                request: Some(request_id.into()),
                name,
            },
            work,
        )
        .await
}

/// Runs `work` as done by `name` (`rule order-placed`, `mapping 7c1e…`): a bus call it makes is marked with the name. Costs a
/// copy of the name.
pub async fn named<F: Future>(name: &str, work: F) -> F::Output {
    let request = CAUSE.try_with(|c| c.request.clone()).ok().flatten();
    CAUSE
        .scope(
            Cause {
                request,
                name: Some(name.into()),
            },
            work,
        )
        .await
}

/// The cause of whatever is running now, if it has one.
#[must_use]
pub fn current() -> Option<Cause> {
    CAUSE.try_with(Clone::clone).ok()
}

/// One finished call on the bus. Text that is not known is empty.
#[derive(Debug, Clone, Copy)]
pub struct BusCall<'a> {
    /// The service that made it (`sns`); empty for a call made as the caller itself ([`crate::Peers::call_as`]).
    pub acting: &'a str,
    /// The target's name in a credential scope.
    pub signing_name: &'static str,
    /// The target's SDK id.
    pub sdk_id: &'static str,
    pub operation: &'static str,
    /// The input member that named what it acted on (`QueueUrl`), and its value; empty when none did.
    pub member: &'static str,
    pub resource: &'a str,
    /// The access key of the caller it was made for.
    pub access_key_id: &'a str,
    pub region: &'a str,
    /// The HTTP status of the answer (200, or the error's).
    pub status: u16,
    /// The error code; empty for a success.
    pub code: &'a str,
    pub took: Duration,
    /// What made it, when known.
    pub cause: Option<&'a Cause>,
}

/// Told about every finished bus call. Called on the caller's task: it must be quick and must not block.
pub trait CallWatcher: Send + Sync + 'static {
    fn called(&self, call: &BusCall<'_>);
}

/// Background reads that run whether or not anything happens (a Lambda event source polling its queue or stream), and that a
/// watcher is not told about when no request from the wire caused them: they would fill the record with nothing.
#[must_use]
pub fn is_poll(signing_name: &str, operation: &str) -> bool {
    matches!(
        (signing_name, operation),
        ("sqs", "ReceiveMessage" | "GetQueueAttributes")
            | (
                "kinesis" | "dynamodb",
                "GetRecords" | "GetShardIterator" | "DescribeStream" | "ListShards"
            )
    )
}

/// A bus call being watched: what [`crate::Peers`] tells the watcher when it is answered. Made only while a watcher is set.
pub(crate) struct Pending {
    watcher: std::sync::Arc<dyn CallWatcher>,
    started: std::time::Instant,
    acting: String,
    signing_name: &'static str,
    sdk_id: &'static str,
    operation: &'static str,
    member: &'static str,
    resource: String,
    access_key_id: String,
    region: String,
    cause: Option<Cause>,
}

impl Pending {
    /// Starts watching the decoded `call` to `service`, unless it is a background poll nobody caused.
    pub(crate) fn begin(
        watcher: std::sync::Arc<dyn CallWatcher>,
        service: &dyn aws_wire::service::Service,
        call: &aws_wire::service::Call,
    ) -> Option<Self> {
        let (meta, tables) = (service.meta(), service.tables());
        let operation = tables.op(call.op).map_or("", |o| o.name);
        let cause = current();
        if cause.as_ref().is_none_or(|c| c.request.is_none())
            && is_poll(meta.signing_name, operation)
            && !is_asked()
        {
            return None;
        }
        let (member, resource) =
            crate::naming::resource_of(tables, &call.arena, &call.body, call.root)
                .unwrap_or_default();
        Some(Self {
            watcher,
            started: std::time::Instant::now(),
            acting: call.caller.service.clone().unwrap_or_default(),
            signing_name: meta.signing_name,
            sdk_id: meta.sdk_id,
            operation,
            member,
            resource: resource.into_owned(),
            access_key_id: call.caller.access_key_id.clone(),
            region: call.caller.region.clone(),
            cause,
        })
    }

    /// Tells the watcher how the call ended.
    pub(crate) fn end(self, failed: Option<&aws_wire::ApiError>) {
        self.watcher.called(&BusCall {
            acting: &self.acting,
            signing_name: self.signing_name,
            sdk_id: self.sdk_id,
            operation: self.operation,
            member: self.member,
            resource: &self.resource,
            access_key_id: &self.access_key_id,
            region: &self.region,
            status: failed.map_or(200, |e| e.status),
            code: failed.map_or("", |e| &e.code),
            took: self.started.elapsed(),
            cause: self.cause.as_ref(),
        });
    }
}
