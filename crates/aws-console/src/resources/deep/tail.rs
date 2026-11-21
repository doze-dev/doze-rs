//! CloudWatch Logs' Live Tail in the browser: one `StartLiveTail` made as the console, its event stream read as it comes
//! ([`super::frames`]) and sent on as server-sent events. The console keeps no timer of its own: the updates are the service's
//! (a `sessionUpdate` a second while the session is open, as AWS sends them). Closing the page drops the stream, which ends the
//! session; so does the console stopping.
//!
//! Events: `start` (the `sessionStart` member, as the model has it), `update` (`sessionUpdate`: `sessionResults`,
//! `sessionMetadata`), `error` (`{"code","message"}`, an exception the stream carried, after which it ends).

use std::convert::Infallible;

use aws_wire::rest::Download;
use axum::response::sse::Event;
use futures_util::stream::{self, Stream};
use serde_json::{Map, Value, json};
use tokio::sync::OwnedSemaphorePermit;
use tokio_util::sync::CancellationToken;

use super::frames::{Message, Reader};

/// What the page asks a session for (its log group is the address's, set by the descriptor's binding).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Ask {
    /// Streams of the group to keep to (none: all of them).
    pub streams: Vec<String>,
    /// A filter pattern (empty: every event).
    pub pattern: String,
}

/// What a tail's address asks for: `stream` any number of times, `pattern` at most once; each with its escapes.
#[must_use]
pub fn ask_of(query: Option<&str>, decode: impl Fn(&str) -> String) -> Ask {
    let mut ask = Ask::default();
    for (key, value) in query
        .unwrap_or("")
        .split('&')
        .filter_map(|p| p.split_once('='))
    {
        let value = decode(value);
        match key {
            "stream" if !value.is_empty() => ask.streams.push(value),
            "pattern" => ask.pattern = value,
            _ => {}
        }
    }
    ask
}

/// The page's part of the `StartLiveTail` request for `ask` (the group, by its ARN, is bound over it).
#[must_use]
pub fn input(ask: &Ask) -> Map<String, Value> {
    let mut body = Map::new();
    if !ask.streams.is_empty() {
        body.insert("logStreamNames".into(), json!(ask.streams));
    }
    if !ask.pattern.trim().is_empty() {
        body.insert("logEventFilterPattern".into(), json!(ask.pattern));
    }
    body
}

/// What one message of the stream is to the page, if anything (the `initial-response` a JSON stream opens with is not).
#[must_use]
pub fn event_of(m: &Message) -> Option<(&'static str, String)> {
    let payload: Value = serde_json::from_slice(&m.payload).unwrap_or(Value::Null);
    match (m.header(":message-type"), m.header(":event-type")) {
        (Some("event"), Some("sessionStart")) => Some(("start", payload.to_string())),
        (Some("event"), Some("sessionUpdate")) => Some(("update", payload.to_string())),
        (Some("event"), _) => None,
        _ => {
            let code = m
                .header(":exception-type")
                .or_else(|| m.header(":error-code"))
                .unwrap_or("InternalError");
            let message = payload
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| m.header(":error-message"))
                .unwrap_or("The session ended.");
            Some((
                "error",
                json!({"code": code, "message": message}).to_string(),
            ))
        }
    }
}

/// The session's events as the page reads them, until the stream ends, breaks, or `stop` is cancelled. `slot` is held as long.
pub fn events(
    download: Download,
    stop: CancellationToken,
    slot: OwnedSemaphorePermit,
) -> impl Stream<Item = Result<Event, Infallible>> {
    struct Open {
        download: Download,
        reader: Reader,
        ready: std::collections::VecDeque<Event>,
        stop: CancellationToken,
        done: bool,
        _slot: OwnedSemaphorePermit,
    }
    let open = Open {
        download,
        reader: Reader::default(),
        ready: std::collections::VecDeque::new(),
        stop,
        done: false,
        _slot: slot,
    };
    stream::unfold(open, |mut o| async move {
        loop {
            if let Some(e) = o.ready.pop_front() {
                return Some((Ok(e), o));
            }
            if o.done {
                return None;
            }
            let chunk = tokio::select! {
                () = o.stop.cancelled() => return None,
                c = std::future::poll_fn(|cx| o.download.as_mut().poll_next(cx)) => c,
            };
            let Some(Ok(bytes)) = chunk else {
                return None;
            };
            match o.reader.push(&bytes) {
                Ok(messages) => {
                    for m in &messages {
                        if let Some((name, data)) = event_of(m) {
                            o.done |= name == "error";
                            o.ready.push_back(Event::default().event(name).data(data));
                        }
                    }
                }
                Err(_) => {
                    o.done = true;
                    let data = json!({"code": "BrokenStream", "message": "The Live Tail stream could not be read."});
                    o.ready
                        .push_back(Event::default().event("error").data(data.to_string()));
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use aws_wire::eventstream::{event, exception};

    use super::{Ask, ask_of, event_of, input};
    use crate::resources::deep::frames::Reader;

    #[test]
    fn a_tail_asks_for_its_streams_and_pattern_and_never_its_group() {
        let unescape = |v: &str| v.replace("%20", " ").replace("%3F", "?");
        let ask = ask_of(
            Some("group=other&stream=web-1&stream=web%202&pattern=%3FERROR%20%3FWARN&x=1"),
            unescape,
        );
        assert_eq!(ask.streams, ["web-1", "web 2"]);
        assert_eq!(ask.pattern, "?ERROR ?WARN");
        let body = input(&ask);
        assert_eq!(body["logStreamNames"][1], "web 2");
        assert_eq!(body["logEventFilterPattern"], "?ERROR ?WARN");
        assert!(body.get("logGroupIdentifiers").is_none());
        assert!(input(&Ask::default()).is_empty());
        assert_eq!(ask_of(None, unescape), Ask::default());
    }

    #[test]
    fn the_stream_is_start_updates_and_an_ending_error() {
        let mut bytes = event("initial-response", "application/json", b"{}");
        bytes.extend(event(
            "sessionStart",
            "application/json",
            b"{\"sessionId\":\"s\"}",
        ));
        bytes.extend(event(
            "sessionUpdate",
            "application/json",
            b"{\"sessionResults\":[]}",
        ));
        bytes.extend(exception(
            "SessionTimeoutException",
            b"{\"message\":\"timed out\"}",
        ));
        let got: Vec<_> = Reader::default()
            .push(&bytes)
            .unwrap_or_default()
            .iter()
            .filter_map(event_of)
            .collect();
        let names: Vec<_> = got.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, ["start", "update", "error"]);
        assert!(
            got.iter()
                .any(|(_, d)| d.contains("SessionTimeoutException") && d.contains("timed out"))
        );
    }
}
