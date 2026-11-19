//! What a finished request was, told to whoever asked to be told ([`crate::Config::observer`]: the console's Live ring).
//!
//! Nothing here runs unless an observer is set. When one is, a request to an AWS API carries a [`Seen`] in its extensions; the
//! pipeline notes on it what it learns on the way (the service and the caller once the service is known, the operation once it
//! is routed, the error code when it fails), and when the answer is made the observer is told once, with borrowed text. The
//! cost on the request path is one small allocation, the copy of the caller's key and region, and the observer's own work.
//!
//! Not told: `/_doze/health`, and the addresses a service or a local hook owns (`/_doze/...`, a Lambda function URL, an API
//! Gateway stage): they are not AWS API calls. A call one service makes to another goes over the peer bus, not through here.

use std::borrow::Cow;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use aws_auth::claim::Claim;
use aws_schema::meta::ServiceMeta;
use aws_wire::{ApiError, RequestId};
use http::request::Parts;

use crate::pipeline::{Entry, Shared};

/// Told about every finished AWS API request. Called on the request path: it must be quick and must not block.
pub trait Observer: std::fmt::Debug + Send + Sync + 'static {
    /// One request has been answered (its head: a streamed body may still be going out).
    fn finished(&self, call: &Finished<'_>);
}

/// One answered request. Text that is not known is empty.
#[derive(Debug, Clone, Copy)]
pub struct Finished<'a> {
    /// The id the answer carries.
    pub request_id: &'a str,
    /// The service's name in a credential scope (`sqs`, `states`); empty when no service of this build was named.
    pub signing_name: &'static str,
    /// The service's SDK id (`SQS`, `SFN`): two services can share a signing name (DynamoDB and its Streams).
    pub sdk_id: &'static str,
    /// The operation (`SendMessage`); empty when the request was refused before one was found.
    pub action: &'static str,
    /// What was acted on, when the gateway itself knows (an S3 path, `bucket/key`); empty otherwise.
    pub resource: &'a str,
    /// The access key id the request was signed with.
    pub access_key_id: &'a str,
    /// The region it was signed for.
    pub region: &'a str,
    /// The HTTP status answered.
    pub status: u16,
    /// The error code answered (`QueueDoesNotExist`); empty for a success.
    pub code: &'a str,
    /// From the request's head being read to its answer being made.
    pub took: Duration,
}

/// What the pipeline has learnt about the request in hand.
#[derive(Default)]
pub(crate) struct Seen(Mutex<Notes>);

#[derive(Default)]
struct Notes {
    meta: Option<&'static ServiceMeta>,
    action: &'static str,
    resource: String,
    access_key_id: String,
    region: String,
    code: Cow<'static, str>,
}

fn note(parts: &Parts, write: impl FnOnce(&mut Notes)) {
    if let Some(seen) = parts.extensions.get::<Arc<Seen>>() {
        write(&mut seen.0.lock().unwrap_or_else(PoisonError::into_inner));
    }
}

/// The request is for `entry`'s service, signed as `claim` says.
pub(crate) fn service(parts: &Parts, entry: &Entry, claim: &Claim) {
    note(parts, |n| {
        n.meta = Some(entry.service.meta());
        n.access_key_id.clone_from(&claim.access_key_id);
        n.region.clone_from(&claim.region);
    });
}

/// The request is operation `op` of `entry`'s model.
pub(crate) fn operation(parts: &Parts, entry: &Entry, op: u32) {
    note(parts, |n| {
        n.action = entry.service.tables().op(op).map_or("", |o| o.name);
    });
}

/// The request acts on `resource`.
pub(crate) fn resource(parts: &Parts, resource: &str) {
    note(parts, |n| resource.clone_into(&mut n.resource));
}

/// The request is answered with `error`.
pub(crate) fn failed(parts: &Parts, error: &ApiError) {
    note(parts, |n| n.code.clone_from(&error.code));
}

/// Starts watching a request, if anyone wants to be told: the time it started.
pub(crate) fn begin(shared: &Shared, parts: &mut Parts) -> Option<Instant> {
    shared.config.observer.as_ref()?;
    parts.extensions.insert(Arc::new(Seen::default()));
    Some(Instant::now())
}

/// Tells the observer about a request [`begin`] started watching, now that its answer is made.
pub(crate) fn end<B>(
    shared: &Shared,
    started: Option<Instant>,
    id: &RequestId,
    parts: &Parts,
    response: &http::Response<B>,
) {
    let (Some(observer), Some(started), Some(seen)) = (
        shared.config.observer.as_ref(),
        started,
        parts.extensions.get::<Arc<Seen>>(),
    ) else {
        return;
    };
    let notes = seen.0.lock().unwrap_or_else(PoisonError::into_inner);
    let status = response.status().as_u16();
    // A refusal made before any service was known (no signature, an unknown service) noted no code: the JSON protocols say it in
    // a header.
    let code = if notes.code.is_empty() && status >= 400 {
        response
            .headers()
            .get("x-amzn-errortype")
            .and_then(|v| v.to_str().ok())
            .map_or("", |v| v.split([':', '#']).next_back().unwrap_or(v))
    } else {
        &notes.code
    };
    observer.finished(&Finished {
        request_id: id.as_str(),
        signing_name: notes.meta.map_or("", |m| m.signing_name),
        sdk_id: notes.meta.map_or("", |m| m.sdk_id),
        action: notes.action,
        resource: &notes.resource,
        access_key_id: &notes.access_key_id,
        region: &notes.region,
        status,
        code,
        took: started.elapsed(),
    });
}
