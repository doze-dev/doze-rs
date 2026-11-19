//! `StartLiveTail`: an event stream (`application/vnd.amazon.eventstream`) that carries the events the named groups take while it is
//! open. As the model documents: one `sessionStart`, then a `sessionUpdate` every second (empty when nothing came), each with at most
//! 500 events (sampled down past that, `sampled: true`), the events a slow reader has not taken buffered to 5,000 and the oldest
//! dropped past it, and a `SessionTimeoutException` after three hours. The session ends when the reader goes away.
//!
//! The actor keeps each open session's filter and hands it the events `PutLogEvents` takes (`Core::tail`); a task per session (only
//! while one is open) writes the frames. A JSON-protocol event stream opens with an `initial-response` event, which botocore reads
//! before the members (memory: botocore's `JSONParser`).

use std::collections::VecDeque;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use aws_wire::ApiError;
use aws_wire::eventstream::{event, exception};
use aws_wire::rest::{ByteStream, Download};
use aws_wire::service::Call;
use bytes::Bytes;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use crate::core::Core;
use crate::events::Incoming;
use crate::pattern::{self, Pattern};
use crate::req::{Req, invalid, name_of_identifier, no_group};

/// The model: "can include as many as 500 log events".
const PER_UPDATE: usize = 500;
/// The model: "buffers up to 10 LiveTailSessionUpdate events or 5000 log events".
const BUFFERED: usize = 5000;
/// The model: sessions "time out after three hours".
const SESSION: Duration = Duration::from_secs(3 * 3600);
/// Sessions open at once in a region (memory: AWS's quota is 15 concurrent Live Tail sessions).
const MAX_SESSIONS: usize = 15;

/// An open session, as the actor holds it.
#[derive(Debug)]
pub(crate) struct Tail {
    groups: Vec<(String, String)>,
    streams: Vec<String>,
    prefixes: Vec<String>,
    pattern: Pattern,
    tx: mpsc::Sender<Vec<Value>>,
}

/// A `StartLiveTail` call for the actor, and where the session start goes.
#[derive(Debug)]
pub(crate) struct TailAsk {
    pub(crate) call: Call,
    pub(crate) events: mpsc::Sender<Vec<Value>>,
    pub(crate) reply: oneshot::Sender<Result<Value, ApiError>>,
}

impl Core {
    /// Opens a session: the `sessionStart` it begins with, or why it is refused. Wording (memory).
    pub(crate) fn open_tail(
        &mut self,
        call: &Call,
        tx: mpsc::Sender<Vec<Value>>,
    ) -> Result<Value, ApiError> {
        let r = Req::new(call);
        let ids = r.strings("logGroupIdentifiers");
        let (streams, prefixes) = (
            r.strings("logStreamNames"),
            r.strings("logStreamNamePrefixes"),
        );
        if !streams.is_empty() && !prefixes.is_empty() {
            return Err(invalid(
                "logStreamNames and logStreamNamePrefixes cannot both be specified.",
            ));
        }
        if (!streams.is_empty() || !prefixes.is_empty()) && ids.len() > 1 {
            return Err(invalid(
                "logStreamNames and logStreamNamePrefixes can be specified only with one log group.",
            ));
        }
        let mut groups = Vec::new();
        for id in &ids {
            if !id.starts_with("arn:") || id.ends_with('*') {
                return Err(invalid(
                    "Each logGroupIdentifier must be a log group ARN that does not end with '*'.",
                ));
            }
            let name = name_of_identifier(id);
            if self.groups.get(&name).is_none() {
                return Err(no_group());
            }
            groups.push((name, id.clone()));
        }
        let text = r.s("logEventFilterPattern");
        let pattern = pattern::parse(text).map_err(crate::filters::refused)?;
        self.tails.retain(|t| !t.tx.is_closed());
        if self.tails.len() >= MAX_SESSIONS {
            return Err(ApiError::modeled(
                "LimitExceededException",
                format!("At most {MAX_SESSIONS} Live Tail sessions can be open at once."),
            ));
        }
        let mut start = json!({
            "requestId": call.id.as_str(),
            "sessionId": aws_seal::uuid_v4().unwrap_or_default(),
            "logGroupIdentifiers": ids,
        });
        for (k, v) in [
            ("logStreamNames", &streams),
            ("logStreamNamePrefixes", &prefixes),
        ] {
            if !v.is_empty() {
                crate::req::Put::put(&mut start, k, json!(v));
            }
        }
        if r.v.get("logEventFilterPattern").is_some() {
            crate::req::Put::put(&mut start, "logEventFilterPattern", json!(text));
        }
        self.tails.push(Tail {
            groups,
            streams,
            prefixes,
            pattern,
            tx,
        });
        Ok(start)
    }

    /// Hands the sessions watching `group` the events of a `PutLogEvents` they select.
    pub(crate) fn tail(&mut self, group: &str, stream: &str, events: &[Incoming], now: i64) {
        if self.tails.is_empty() {
            return;
        }
        self.tails.retain(|t| !t.tx.is_closed());
        for t in &self.tails {
            let Some((_, id)) = t.groups.iter().find(|(g, _)| g == group) else {
                continue;
            };
            let wanted = (t.streams.is_empty() || t.streams.iter().any(|s| s == stream))
                && (t.prefixes.is_empty()
                    || t.prefixes.iter().any(|p| stream.starts_with(p.as_str())));
            if !wanted {
                continue;
            }
            let picked: Vec<Value> = events
                .iter()
                .filter(|e| t.pattern.matches(&e.message))
                .map(|e| {
                    json!({"logStreamName": stream, "logGroupIdentifier": id, "message": e.message,
                           "timestamp": e.ts, "ingestionTime": now})
                })
                .collect();
            if !picked.is_empty() && t.tx.try_send(picked).is_err() {
                // WHY: a session whose buffer is full drops what it cannot take, as AWS drops the oldest past its buffer.
            }
        }
    }
}

/// The frames of a session, as the gateway pulls them.
struct Frames(mpsc::Receiver<io::Result<Bytes>>);

impl ByteStream for Frames {
    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<io::Result<Bytes>>> {
        self.0.poll_recv(cx)
    }
}

/// The answer's body: `initial-response`, `sessionStart`, then an update a second until the reader leaves or the session times out.
pub(crate) fn stream(
    tasks: &doze_runtime::Tasks,
    start: Value,
    mut events: mpsc::Receiver<Vec<Value>>,
) -> Download {
    let (tx, rx) = mpsc::channel::<io::Result<Bytes>>(16);
    let stopping = tasks.stopping();
    tasks.spawn(async move {
        let send = |frame: Vec<u8>| tx.send(Ok(Bytes::from(frame)));
        if send(event("initial-response", "application/json", b"{}")).await.is_err()
            || send(event("sessionStart", "application/json", start.to_string().as_bytes())).await.is_err()
        {
            return;
        }
        let ends = tokio::time::Instant::now() + SESSION;
        let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + Duration::from_secs(1), Duration::from_secs(1));
        let mut held: VecDeque<Value> = VecDeque::new();
        loop {
            tokio::select! {
                () = stopping.cancelled() => return,
                () = tx.closed() => return,
                got = events.recv() => match got {
                    Some(batch) => hold(&mut held, batch),
                    None => return,
                },
                _ = tick.tick() => {
                    if tokio::time::Instant::now() >= ends {
                        let body = json!({"message": "Live Tail session has timed out after three hours."});
                        drop(send(exception("SessionTimeoutException", body.to_string().as_bytes())).await);
                        return;
                    }
                    if send(update(&mut held)).await.is_err() {
                        return;
                    }
                }
            }
        }
    });
    Box::pin(Frames(rx))
}

fn hold(held: &mut VecDeque<Value>, batch: Vec<Value>) {
    held.extend(batch);
    while held.len() > BUFFERED {
        held.pop_front();
    }
}

/// One `sessionUpdate`: what came since the last, sampled down to 500 when more came (an even spread across them).
fn update(held: &mut VecDeque<Value>) -> Vec<u8> {
    let all: Vec<Value> = held.drain(..).collect();
    let sampled = all.len() > PER_UPDATE;
    let results: Vec<Value> = if sampled {
        let step = all.len() as f64 / PER_UPDATE as f64;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        (0..PER_UPDATE)
            .filter_map(|i| all.get((i as f64 * step) as usize).cloned())
            .collect()
    } else {
        all
    };
    let body = json!({"sessionMetadata": {"sampled": sampled}, "sessionResults": results});
    event(
        "sessionUpdate",
        "application/json",
        body.to_string().as_bytes(),
    )
}
