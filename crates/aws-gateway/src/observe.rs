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
use aws_peers::summary::{self, Summary, TreeFields};
use aws_schema::meta::ServiceMeta;
use aws_wire::service::{Call, Caller};
use aws_wire::{ApiError, RequestId};
use bytes::Bytes;
use http::request::Parts;

use crate::fault::{Ask, Done, Faults, LabNote, Tag, Verdict};
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
    /// The identity policy that allowed the request, when IAM is on and one did ([`crate::Config::would_deny`]); none for a request
    /// IAM refused, one the account root made, and one only a resource-based policy allowed.
    pub allowed_by: Option<AllowedBy<'a>>,
    /// The request's `User-Agent` header as sent (an SDK names itself and its version there); empty when it had none. The
    /// observer keeps what it needs of it: the gateway neither reads nor stores it.
    pub user_agent: &'a str,
    /// The words an error answer carries (the service's own: the rule that was broken), cut at [`MOST_MESSAGE`] bytes; empty for a
    /// success and for a refusal made before a service read the request.
    pub message: &'a str,
    /// What a function invocation answered in `X-Amz-Function-Error` (`Unhandled`, `Handled`): the call itself succeeded, the
    /// function failed. Empty otherwise.
    pub function_error: &'a str,
    /// The length of the answer when it is known before it is sent (a whole body); `None` for a streamed one.
    pub answer_bytes: Option<u32>,
    /// What the call did, read from its input and its answer ([`aws_peers::summary`]); none for an answer that failed.
    pub did: Did<'a>,
    /// The Lab rule that touched the request ([`crate::Config::faults`]); a dropped request has status 0.
    pub lab: Option<LabNote<'a>>,
}

/// The identity policy that let a request through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllowedBy<'a> {
    /// The policy's name.
    pub policy: &'a str,
    /// Where it is attached, as IAM's `PolicySourceType` words it: `aws-managed`, `user-managed`, `user`, `group` or `role`.
    pub kind: &'a str,
}

/// The most bytes of an error message the observer is told.
pub const MOST_MESSAGE: usize = 240;

mod did;
mod flight;

pub use did::Did;
pub(crate) use did::{output, output_json};
pub use flight::WouldDeny;

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
    message: String,
    would_be_denied: bool,
    /// The identity policy that allowed the request, and its kind ([`WouldDeny::allowed_by`]).
    allowed_by: Option<(String, String)>,
    summary: Summary,
    detail: String,
    /// The Lab rule that touched the request, and what it did.
    lab: Option<(Tag, Done)>,
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
            value.as_ref().clone_into(&mut n.resource);
        }
        let (sdk_id, action) = (entry.service.meta().sdk_id, n.action);
        let fields = TreeFields {
            tables: entry.service.tables(),
            arena: &call.arena,
            body: &call.body,
            root: call.root,
        };
        let Notes {
            summary, detail, ..
        } = &mut *n;
        summary::from_input(sdk_id, action, &fields, summary, detail);
    });
}

/// The request's input was refused when it was decoded (a name the model's pattern does not allow): there is no decoded input to read
/// what it acted on from, so the member that names it is read from the JSON body as it came, and a refused call still says what it
/// named (`pick lists!`). Nothing is read when the call already named a resource, or the body is not a JSON object.
pub(crate) fn refused_input(parts: &Parts, body: &[u8]) {
    note(parts, |n| {
        if !n.resource.is_empty() {
            return;
        }
        let Ok(serde_json::Value::Object(map)) = serde_json::from_slice::<serde_json::Value>(body)
        else {
            return;
        };
        for member in aws_peers::naming::MEMBERS {
            let named = map
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(member))
                .and_then(|(_, v)| v.as_str())
                .filter(|v| !v.is_empty());
            if let Some(value) = named {
                n.member = member;
                value.clone_into(&mut n.resource);
                return;
            }
        }
    });
}

/// The request's body decoded ([`crate::pipeline::decode_call`]) and noted: what its input names is what the call acted on
/// ([`input`]); when decoding refuses it, the member that names it is read from the body as it came (`Bytes` is counted, not
/// copied), so a refusal still says what it was about.
pub(crate) fn decode(
    parts: &Parts,
    entry: &Entry,
    (op, id): (u32, &RequestId),
    caller: Caller,
    bytes: Bytes,
) -> Result<Call, ApiError> {
    let sent = bytes.clone();
    let call = crate::pipeline::decode_call(entry, op, id, caller, bytes)
        .inspect_err(|_| refused_input(parts, &sent))?;
    input(parts, entry, &call);
    Ok(call)
}

/// The request is answered with `error`.
pub(crate) fn failed(parts: &Parts, error: &ApiError) {
    note(parts, |n| {
        n.code.clone_from(&error.code);
        let mut end = error.message.len().min(MOST_MESSAGE);
        while !error.message.is_char_boundary(end) {
            end -= 1;
        }
        n.message = error.message[..end].to_owned();
    });
}

/// The request has been judged by a Lab rule, which did `done` to it.
pub(crate) fn lab(parts: &Parts, tag: &Tag, done: Done) {
    note(parts, |n| n.lab = Some((tag.clone(), done)));
}

/// What the Lab's rules say of the request in hand, from what the pipeline has noted of it. A request whose service or operation is
/// not known is passed.
pub(crate) fn judge(parts: &Parts, faults: &dyn Faults) -> Verdict {
    let Some(seen) = parts.extensions.get::<Arc<Seen>>() else {
        return Verdict::Pass;
    };
    let n = seen.notes();
    let Some(meta) = n.meta.filter(|_| !n.action.is_empty()) else {
        return Verdict::Pass;
    };
    faults.judge(&Ask {
        signing_name: meta.signing_name,
        sdk_id: meta.sdk_id,
        operation: n.action,
        resource: &n.resource,
        access_key_id: &n.access_key_id,
        region: &n.region,
        user_agent: parts
            .headers
            .get(http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default(),
    })
}

/// Starts watching a request, if anyone wants to be told (an observer) or to judge it (the Lab's rules): the time it started.
pub(crate) fn begin(shared: &Shared, id: &RequestId, parts: &mut Parts) -> Option<Instant> {
    let observed = shared.config.observer.is_some();
    if !observed && shared.config.faults.is_none() {
        return None;
    }
    let seen = Arc::new(Seen::default());
    if let (true, Some(flight)) = (observed, &shared.config.would_deny) {
        id.as_str().clone_into(&mut seen.notes().request_id);
        flight.enter(&seen);
    }
    parts.extensions.insert(seen);
    Some(Instant::now())
}

/// The policy that allowed the request, unless the answer is an access denial: another check of the same request refused it, and a
/// policy that allowed one thing it did must not be named for a call that failed for want of another.
fn allowed_by<'a>(notes: &'a Notes, code: &str) -> Option<AllowedBy<'a>> {
    let (policy, kind) = notes.allowed_by.as_ref()?;
    (!code.starts_with("AccessDenied")).then_some(AllowedBy { policy, kind })
}

/// Tells the observer about a request [`begin`] started watching, now that its answer is made.
pub(crate) fn end<B: hyper::body::Body>(
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
    // A dropped request was answered with nothing: it has no status.
    let status = if crate::fault::is_dropped(response) {
        0
    } else {
        response.status().as_u16()
    };
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
        allowed_by: allowed_by(&notes, code),
        user_agent: parts
            .headers
            .get(http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default(),
        message: &notes.message,
        function_error: header("x-amz-function-error").unwrap_or_default(),
        answer_bytes: response
            .body()
            .size_hint()
            .exact()
            .and_then(|n| u32::try_from(n).ok()),
        // A failed call did nothing a summary could say.
        did: if status != 0 && status < 400 {
            Did {
                summary: notes.summary,
                detail: &notes.detail,
            }
        } else {
            Did::NONE
        },
        lab: notes.lab.as_ref().map(|(tag, done)| LabNote {
            rule: tag.rule,
            name: &tag.name,
            done: *done,
        }),
    });
}
