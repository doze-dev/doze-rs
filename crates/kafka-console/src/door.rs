//! The console's door to other machines, decided on every request (SEC-250).
//!
//! The start-up rule (SEC-188) lets a console be served beyond loopback without `--console-unprotected` only in front of a
//! broker that nothing guards: no SASL, no TLS, no ACL, Kafka itself on the network. Of those, only the ACLs can change while
//! the broker runs, so the rule is held for the life of the process here: while the broker has any ACL, such a console answers
//! no API request from another machine (403 `CONSOLE_CLOSED_TO_NETWORK`), ends the tails it is serving to one, and still serves
//! the page and its files, so the page can say why. A request from this machine (a loopback peer) is answered as before. When
//! every ACL is deleted the door opens again: the rule is "never wider than the broker", evaluated live. With
//! `--console-unprotected` there is no door: the person accepted the exposure.
//!
//! The check is one atomic load ([`kafka_broker::Broker::acl_count`]); a change is counted before its CreateAcls or DeleteAcls
//! is answered, so once a client has the answer the door is already closed (or open).

use std::net::{IpAddr, SocketAddr};

use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::middleware::Next;
use axum::response::Response;
use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::Backend;
use crate::source::Source;
use crate::state::Shared;

/// The code of the refusal, which the page matches to show its explanation.
pub const CLOSED: &str = "CONSOLE_CLOSED_TO_NETWORK";

/// A console that closes to other machines while the broker has ACLs: served beyond loopback, allowed at start only because
/// nothing guarded the broker, and not `--console-unprotected`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Door {
    /// Where the console can be used from this machine (`http://127.0.0.1:9093/`), when it listens on loopback too.
    pub local: Option<String>,
}

/// What the door does with one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Answer,
    Refuse,
}

/// The rule: a watched console with ACLs in force refuses a peer that is not this machine (no peer known is not this machine).
/// Pure, so the table of cases is the test.
#[must_use]
pub fn verdict(watched: bool, peer: Option<IpAddr>, acls: usize) -> Verdict {
    if !watched || acls == 0 || this_machine(peer) {
        Verdict::Answer
    } else {
        Verdict::Refuse
    }
}

/// Whether the peer is this machine: a loopback address (`::ffff:127.0.0.1` too). No peer known is not.
fn this_machine(peer: Option<IpAddr>) -> bool {
    peer.is_some_and(|ip| ip.to_canonical().is_loopback())
}

/// The words of the refusal: what happened, why, and the two ways forward.
#[must_use]
pub fn words(door: &Door) -> String {
    let here = match &door.local {
        Some(url) => format!("use it on the machine doze-kafka runs on, at {url}"),
        None => "serve it on loopback (--console-addr 127.0.0.1:PORT) and use it on the machine doze-kafka runs on".to_owned(),
    };
    format!(
        "This broker now has ACLs, so the console no longer answers other machines: it has no login and acts as a super user, so it would get round them. To go on, {here}, or restart doze-kafka with --console-unprotected to accept that."
    )
}

/// What the log says when the count of ACLs goes from `before` to `now`, if the door changed: once when it closes, once when
/// it opens again.
#[must_use]
pub fn said(before: usize, now: usize) -> Option<&'static str> {
    match (before, now) {
        (0, n) if n > 0 => Some(
            "the broker now has ACLs, so the console no longer answers other machines (it has no login and acts as a super user): use it from this machine, or restart with --console-unprotected to accept that",
        ),
        (b, 0) if b > 0 => Some(
            "the broker has no ACL now, so the console answers other machines again, as the broker does",
        ),
        _ => None,
    }
}

/// The door of this console and the backend whose ACLs it watches, if it has one.
fn watched(s: &Shared) -> Option<(&Door, &Backend)> {
    match &s.source {
        Source::Live(live) => live.door.as_ref().map(|d| (d, &live.backend)),
        #[cfg(any(test, feature = "examples"))]
        Source::Fixture => None,
    }
}

/// The door in front of the API: refuses an API request from another machine while the broker has ACLs, and ends a tail it
/// was serving to one once ACLs appear.
pub async fn door_guard(State(s): State<Shared>, req: Request, next: Next) -> Response {
    let Some((door, backend)) = watched(&s) else {
        return next.run(req).await;
    };
    let api = s
        .web
        .strip(req.uri().path())
        .is_some_and(|rest| rest == "/api" || rest.starts_with("/api/"));
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());
    if !api || this_machine(peer) {
        return next.run(req).await;
    }
    if verdict(true, peer, backend.acl_count()) == Verdict::Refuse {
        return crate::api::error(StatusCode::FORBIDDEN, CLOSED, &words(door));
    }
    let res = next.run(req).await;
    let stream = res
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/event-stream"));
    if !stream {
        return res;
    }
    // A tail outlives the request that opened it: it ends before the first chunk it would send once the broker has ACLs.
    let backend = backend.clone();
    let (parts, body) = res.into_parts();
    let cut = body
        .into_data_stream()
        .take_while(move |_| std::future::ready(backend.acl_count() == 0));
    Response::from_parts(parts, Body::from_stream(cut))
}

/// Logs, once each, when the door closes and when it opens again, until `stop`.
pub fn watch(backend: &Backend, stop: CancellationToken) {
    let mut changes = backend.acl_changes();
    let mut before = *changes.borrow_and_update();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                () = stop.cancelled() => return,
                changed = changes.changed() => if changed.is_err() { return },
            }
            let now = *changes.borrow_and_update();
            if let Some(line) = said(before, now) {
                tracing::warn!("{line}");
            }
            before = now;
        }
    });
}

#[cfg(test)]
mod tests;
