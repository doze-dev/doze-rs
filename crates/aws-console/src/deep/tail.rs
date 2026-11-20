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
use serde_json::{Value, json};
use tokio::sync::OwnedSemaphorePermit;
use tokio_util::sync::CancellationToken;

use super::frames::{Message, Reader};

/// What the page asks a session for.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Ask {
    /// The log group's name.
    pub group: String,
    /// Streams of the group to keep to (none: all of them).
    pub streams: Vec<String>,
    /// A filter pattern (empty: every event).
    pub pattern: String,
}

/// The `StartLiveTail` request for `ask`, in `region` of `account` (the model takes a group by its ARN).
#[must_use]
pub fn request(ask: &Ask, (account, region): (&str, &str)) -> Value {
    let mut body = json!({
        "logGroupIdentifiers": [format!("arn:aws:logs:{region}:{account}:log-group:{}", ask.group)],
    });
    if !ask.streams.is_empty() {
        body.as_object_mut()
            .map(|o| o.insert("logStreamNames".into(), json!(ask.streams)));
    }
    if !ask.pattern.trim().is_empty() {
        body.as_object_mut()
            .map(|o| o.insert("logEventFilterPattern".into(), json!(ask.pattern)));
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

    use super::{Ask, event_of, request};
    use crate::deep::frames::Reader;

    #[test]
    fn a_group_is_asked_for_by_its_arn_with_its_streams_and_pattern() {
        let ask = Ask {
            group: "/app/api".into(),
            streams: vec!["web-1".into()],
            pattern: "ERROR".into(),
        };
        let body = request(&ask, ("111122223333", "eu-west-1"));
        assert_eq!(
            body["logGroupIdentifiers"][0],
            "arn:aws:logs:eu-west-1:111122223333:log-group:/app/api"
        );
        assert_eq!(body["logStreamNames"][0], "web-1");
        assert_eq!(body["logEventFilterPattern"], "ERROR");
        let bare = request(
            &Ask {
                group: "g".into(),
                ..Ask::default()
            },
            ("1", "r"),
        );
        assert!(
            bare.get("logStreamNames").is_none() && bare.get("logEventFilterPattern").is_none()
        );
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
