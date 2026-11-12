//! What a fault plan does to one request: asked before the broker is, acted on around it. Nothing here runs when no plan is installed
//! (`judge` is one `Option` test), and nothing here touches the broker's state.

use std::sync::Arc;
use std::time::Duration;

use kafka_storage::fs::Fs;
use kafka_wire::api::Request;
use kafka_wire::header::RequestHead;
use tokio_util::sync::CancellationToken;

use super::{Answer, Ears, End, Seen, Watch};
use crate::fault::{Act, Applied, Ask, Note, Verdict, fail_response};
use crate::observer::Pending;
use crate::session::steps::Fate;

/// What the plan says about a request, if there is a plan and it says anything.
pub(super) fn judge<F: Fs>(
    ears: &Ears<'_, F>,
    head: &RequestHead,
    request: Option<&Request>,
) -> Option<Act> {
    let plan = ears.ctx.fault.as_ref()?;
    let broker = &ears.ctx.broker;
    let names = |id| broker.topic_name(id);
    let ask = Ask {
        conn: ears.watch.map(|w| w.conn.0),
        principal: ears.caller.principal(),
        head,
        request,
        names: &names,
    };
    match plan.judge(&ask) {
        Verdict::Pass => None,
        Verdict::Act(act) => Some(act),
    }
}

/// Ends an observed request whose connection a rule closed.
pub(super) fn dropped(watch: Option<&Watch>, pending: Option<Pending>, note: &Note) -> End {
    if let Some((w, p)) = watch.zip(pending) {
        w.end(p, Seen::Closed(&End::Faulted.to_string()), 0, Some(note));
    }
    End::Faulted
}

/// The answer a failing rule makes; none for an API the lab cannot answer.
fn failed(
    head: &RequestHead,
    request: &Request,
    code: i16,
    note: Arc<Note>,
) -> Option<Answer> {
    let response = fail_response(request, head.version, code)?;
    Some(Answer {
        id: head.correlation_id,
        version: head.version,
        response,
        fate: Fate::Open,
        fault: Some(note),
    })
}

/// Waits `delay`; a stop does not wait for it.
pub(super) async fn wait(delay: Duration, stop: &CancellationToken) -> bool {
    tokio::select! {
        () = tokio::time::sleep(delay) => true,
        () = stop.cancelled() => false,
    }
}

/// Whether the request is a Produce that asked for no acknowledgement: a rule that fails it answers nothing either.
pub(super) fn wants_no_answer(request: &Request) -> bool {
    matches!(request, Request::Produce(r) if r.acks == 0)
}


/// A request with no body to answer a code to (ApiVersions): a rule may delay it or close the connection, and no more. `Err` is the
/// close; `Ok` holds the rule that delayed it, if one did, and the request's observer token back.
pub(super) async fn bend<F: Fs>(
    ears: &Ears<'_, F>,
    head: &RequestHead,
    pending: Option<Pending>,
    stop: &CancellationToken,
) -> Result<(Option<Arc<Note>>, Option<Pending>), End> {
    let Some(act) = judge(ears, head, None) else {
        return Ok((None, pending));
    };
    match act.effect {
        Applied::Drop => Err(dropped(ears.watch, pending, &act.note)),
        Applied::Delay(d) if !wait(d, stop).await => Err(super::closed(ears.watch, pending, End::Stopped)),
        Applied::Delay(_) => Ok((Some(act.note), pending)),
        Applied::Fail(_) => Ok((None, pending)),
    }
}

/// What a plan makes of a request the broker is about to handle.
pub(super) enum Plan {
    /// Hand it to the broker; if a rule delays the answer, for how long.
    Serve(Option<Act>),
    /// Answer it with this (a rule failed it), without the broker.
    Answer(Box<Answer>),
    /// Answer nothing (a rule failed a Produce that asked for no acknowledgement).
    Silent(Arc<Note>),
    /// Close the connection.
    Close(Arc<Note>),
}

/// Asks the plan about a request that has a body.
pub(super) fn plan<F: Fs>(ears: &Ears<'_, F>, head: &RequestHead, request: &Request) -> Plan {
    let Some(act) = judge(ears, head, Some(request)) else {
        return Plan::Serve(None);
    };
    match act.effect {
        Applied::Drop => Plan::Close(act.note),
        Applied::Delay(_) => Plan::Serve(Some(act)),
        Applied::Fail(_) if wants_no_answer(request) => Plan::Silent(act.note),
        Applied::Fail(code) => match failed(head, request, code, Arc::clone(&act.note)) {
            Some(answer) => Plan::Answer(Box::new(answer)),
            None => Plan::Serve(None),
        },
    }
}
