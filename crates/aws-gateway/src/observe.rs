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
    /// The input member that named what was acted on (`QueueUrl`, from [`aws_peers::naming::MEMBERS`]); empty for an S3 path,
    /// which the gateway reads from the address, and when nothing was named.
    pub member: &'static str,
    /// What was acted on: an S3 path (`bucket/key`), or the value of `member`; empty when it is not known.
    pub resource: &'a str,
    /// The access key id the request was signed with.
    pub access_key_id: &'a str,
    /// The region it was signed for.
    pub region: &'a str,
    /// The HTTP status answered.
    pub status: u16,
    /// The error code answered as an SDK reads it (`AWS.SimpleQueueService.NonExistentQueue` for SQS, whose JSON answer carries
    /// its query-compatible code in `x-amzn-query-error`); empty for a success.
    pub code: &'a str,
    /// From the request's head being read to its answer being made.
    pub took: Duration,
    /// IAM in soft mode found it would refuse this request and let it through ([`crate::Config::would_deny`]).
    pub would_be_denied: bool,
}

/// The requests in flight, so a refusal IAM's soft mode lets through can be marked on the request it was for ([`crate::Config::would_deny`]).
///
/// IAM is told only the caller and the action, not which request asked, so a refusal is matched by the caller's key and region,
/// then by the operation (`sqs:SendMessage` to `SendMessage`). When the caller has one request in flight it is that one, whatever
/// the action is called (S3's `ListObjectsV2` asks for `s3:ListBucket`); when it has several, those whose operation is the
/// action's are marked; when none is, none is (a guess would mark a call that was allowed).
#[derive(Debug, Default)]
pub struct WouldDeny(Mutex<Vec<Arc<Seen>>>);

impl WouldDeny {
    /// Soft mode let `access_key_id`, signed for `region`, do `action` (`sqs:SendMessage`), which its policies refuse.
    pub fn let_through(&self, access_key_id: &str, region: &str, action: &str) {
        let operation = action.split_once(':').map_or(action, |(_, op)| op);
        let flight = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let mine: Vec<&Arc<Seen>> = flight
            .iter()
            .filter(|s| {
                let n = s.notes();
                n.access_key_id == access_key_id && n.region == region
            })
            .collect();
        let named: Vec<&&Arc<Seen>> = mine
            .iter()
            .filter(|s| s.notes().action == operation)
            .collect();
        if named.is_empty() {
            if let [only] = mine.as_slice() {
                only.notes().would_be_denied = true;
            }
            return;
        }
        for s in named {
            s.notes().would_be_denied = true;
        }
    }

    /// The id of the request `access_key_id`, signed for `region`, is being answered now, when it has exactly one in flight: what
    /// caused a call between services made for that caller on a task the request's own does not reach (a service's actor). With
    /// none in flight, or several, `None`: a guess could name the wrong one.
    #[must_use]
    pub fn request_of(&self, access_key_id: &str, region: &str) -> Option<String> {
        let flight = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let mut mine = flight.iter().filter(|s| {
            let n = s.notes();
            n.access_key_id == access_key_id && n.region == region
        });
        let only = mine.next()?;
        if mine.next().is_some() {
            return None;
        }
        let id = only.notes().request_id.clone();
        (!id.is_empty()).then_some(id)
    }

    fn enter(&self, seen: &Arc<Seen>) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Arc::clone(seen));
    }

    fn leave(&self, seen: &Arc<Seen>) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|s| !Arc::ptr_eq(s, seen));
    }
}

/// What the pipeline has learnt about the request in hand.
#[derive(Debug, Default)]
pub(crate) struct Seen(Mutex<Notes>);

impl Seen {
    fn notes(&self) -> std::sync::MutexGuard<'_, Notes> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[derive(Debug, Default)]
struct Notes {
    /// Kept only while the request is in flight under a [`WouldDeny`].
    request_id: String,
    meta: Option<&'static ServiceMeta>,
    action: &'static str,
    member: &'static str,
    resource: String,
    access_key_id: String,
    region: String,
    code: Cow<'static, str>,
    would_be_denied: bool,
}

fn note(parts: &Parts, write: impl FnOnce(&mut Notes)) {
    if let Some(seen) = parts.extensions.get::<Arc<Seen>>() {
        write(&mut seen.notes());
    }
}

/// The request is signed by `claim`, whether or not a service of this build is signed for.
pub(crate) fn signed(parts: &Parts, claim: &Claim) {
    note(parts, |n| {
        n.access_key_id.clone_from(&claim.access_key_id);
        n.region.clone_from(&claim.region);
    });
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

/// The request's decoded input is `call`: what it acts on is named by the member of it that names a resource, unless the
/// address already said (an S3 path).
pub(crate) fn input(parts: &Parts, entry: &Entry, call: &aws_wire::service::Call) {
    note(parts, |n| {
        if n.resource.is_empty()
            && let Some((member, value)) = aws_peers::naming::resource_of(
                entry.service.tables(),
                &call.arena,
                &call.body,
                call.root,
            )
        {
            n.member = member;
            value.clone_into(&mut n.resource);
        }
    });
}

/// The request is answered with `error`.
pub(crate) fn failed(parts: &Parts, error: &ApiError) {
    note(parts, |n| n.code.clone_from(&error.code));
}

/// Starts watching a request, if anyone wants to be told: the time it started.
pub(crate) fn begin(shared: &Shared, id: &RequestId, parts: &mut Parts) -> Option<Instant> {
    shared.config.observer.as_ref()?;
    let seen = Arc::new(Seen::default());
    if let Some(flight) = &shared.config.would_deny {
        id.as_str().clone_into(&mut seen.notes().request_id);
        flight.enter(&seen);
    }
    parts.extensions.insert(seen);
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
    if let Some(flight) = &shared.config.would_deny {
        flight.leave(seen);
    }
    let notes = seen.notes();
    let status = response.status().as_u16();
    let header = |name: &str| response.headers().get(name).and_then(|v| v.to_str().ok());
    // A query-compatible service (SQS) answers its JSON with the code its query protocol had, which is the code an SDK reports.
    let query = (status >= 400)
        .then(|| header("x-amzn-query-error"))
        .flatten()
        .and_then(|v| v.split(';').next())
        .filter(|c| !c.is_empty());
    // A refusal made before any service was known (no signature, an unknown service) noted no code: the JSON protocols say it in
    // a header.
    let code = match query {
        Some(code) => code,
        None if notes.code.is_empty() && status >= 400 => {
            header("x-amzn-errortype").map_or("", |v| v.split([':', '#']).next_back().unwrap_or(v))
        }
        None => &notes.code,
    };
    observer.finished(&Finished {
        request_id: id.as_str(),
        signing_name: notes.meta.map_or("", |m| m.signing_name),
        sdk_id: notes.meta.map_or("", |m| m.sdk_id),
        action: notes.action,
        member: notes.member,
        resource: &notes.resource,
        access_key_id: &notes.access_key_id,
        region: &notes.region,
        status,
        code,
        took: started.elapsed(),
        would_be_denied: notes.would_be_denied,
    });
}
