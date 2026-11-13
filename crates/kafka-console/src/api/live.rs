//! The Live endpoints of the React console (slice R3): the tape as a snapshot, one request in full (with the rules behind a
//! refusal, decided by `live_words`), the clients, the first request of the newest client, the docks
//! of a topic and of a group, and the one stream: `GET /api/v1/tail/live`, the tape's tail as JSON (CON-1250 to CON-1262).
//!
//! The tail is the classic stream's rules (`stream.rs`) with JSON rows: the server looks once per `pace` (1 to 30 s, clamped
//! here, whatever the client asks), sends an event only when rows were added or a folded row grew, and names each event by a
//! cursor `<epoch>.<version>`. The browser's reader sends the cursor back as `Last-Event-ID` when it reconnects, so it gets the
//! rows it missed and none twice. The epoch is this process's: after a restart the recorder starts empty and its versions start
//! again, so a cursor of another epoch is answered with `reset` and the rows of the new life (never a row of the old one twice).

use std::convert::Infallible;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::stream::{self, Stream, StreamExt};
use serde::Serialize;

use super::json;
use crate::live_words::{
    Kind as RequestKind, Limits as RequestLimits, Rule as RequestRule, fenced_rules,
    fetch_commands, keyed_produce, refused_rules, too_large_rules,
};
use crate::model::{
    ClientsView, FirstView, GroupWireView, LiveState, LiveView, WireClass, WireDetail, WireQuery,
    WireRow, WireWindow,
};
use crate::state::Shared;

/// The most rows the React page asks for in a snapshot (the tail keeps 200 in the page).
pub const SNAPSHOT_MOST: usize = 100;

/// This process's epoch: the time it first answered a Live request, in milliseconds. A cursor carries it.
fn epoch() -> u64 {
    static EPOCH: OnceLock<u64> = OnceLock::new();
    *EPOCH.get_or_init(|| u64::try_from(crate::source::now_ms()).unwrap_or(0))
}

/// The cursor a tail is told to start from, or that names an event: `<epoch>.<version>`.
#[must_use]
pub fn cursor(version: u64) -> String {
    format!("{}.{version}", epoch())
}

/// The version a cursor names, if it is of this process; `None` for a cursor of another life (or no cursor at all).
#[must_use]
pub fn version_of(cursor: &str) -> Option<u64> {
    let (e, v) = cursor.trim().split_once('.')?;
    (e.parse::<u64>().ok()? == epoch()).then_some(())?;
    v.parse().ok()
}

/// The words of the Live page that Rust decides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LiveWords {
    /// What an empty tape says when no filter is on: the command that sends a first request.
    pub empty: String,
    /// The filters that are on, one phrase each (`app is orders`), for the empty tape under filters.
    pub phrases: Vec<String>,
    /// The state's name when the page is in one the boards name (`Nothing yet`, `A busy minute`).
    pub state: Option<String>,
}

/// `GET /api/v1/live`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LiveResponse {
    pub live: LiveView,
    pub words: LiveWords,
    /// Where a tail of this snapshot starts: the cursor of the recorder's version when it was made.
    pub tail_from: String,
}

/// The two commands that read the one record a Fetch asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FetchCopy {
    pub topic: String,
    pub partition: i32,
    pub offset: i64,
    pub kcat: String,
    pub console: String,
}

/// `GET /api/v1/live/request/{id}`: one request in full, which Sheet it opens, and the rules behind a refusal.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RequestResponse {
    pub request: WireDetail,
    pub kind: RequestKind,
    /// What the broker says of the topic now (read when the Sheet opens).
    pub limits: RequestLimits,
    /// The rules list of a refusal (empty for a request that was answered).
    pub rules: Vec<RequestRule>,
    /// A Fetch of one record: how to read it again.
    pub fetch: Option<FetchCopy>,
    /// An INVALID_RECORD on a compacted topic: the command that sends the record with a key.
    pub keyed: Option<String>,
}

/// `GET /api/v1/live/request/{id}`: the request, or `null` and the words that say it fell off the tape. A 200 either way: a
/// deep link to a row the recorder no longer keeps is an ordinary moment, and a 404 is an error line in every browser's console.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RequestAnswer {
    pub found: Option<RequestResponse>,
    pub gone: Option<String>,
}

/// What a request that is not on the tape says.
pub const GONE: &str = "That request is no longer on the wire. The recorder keeps a bounded tape, and this row has fallen off it.";

/// `GET /api/v1/live/first`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FirstResponse {
    /// The newest client the broker had not heard from before; `null` while no client has spoken.
    pub first: Option<FirstView>,
}

/// `GET /api/v1/live/topic-wire?topic=`: the last requests that touched a topic.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TopicWireView {
    pub topic: String,
    pub rows: Vec<WireRow>,
    pub window: WireWindow,
    pub now_ms: i64,
}

/// One event of the tail: the rows that were added or grew since the last, newest first, and the cursor after them.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TailBatch {
    pub cursor: String,
    pub rows: Vec<WireRow>,
}

/// The first event of a tail: it is open, from where, and whether the client's tape is of another life (drop it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TailReady {
    pub cursor: String,
    pub reset: bool,
    /// The seconds between two looks the server gives this tail (what was asked, within 1 to 30).
    pub pace: u64,
}

fn fields(query: Option<&str>) -> Vec<(String, String)> {
    crate::form::parse(query.unwrap_or(""))
}

/// The filters of the React page: the classic ones, with the page's own size.
fn query_of(fields: &[(String, String)]) -> WireQuery {
    let mut q = WireQuery::parse(fields);
    let asked = crate::form::get(fields, "limit").and_then(|v| v.trim().parse::<usize>().ok());
    q.limit = asked.map_or(SNAPSHOT_MOST, |n| n.clamp(1, SNAPSHOT_MOST));
    q
}

/// The filters that are on, one phrase each.
fn phrases(q: &WireQuery) -> Vec<String> {
    let list =
        |what: &str, vs: &[String]| (!vs.is_empty()).then(|| format!("{what} {}", vs.join(" or ")));
    let mut out: Vec<String> = [
        list("app is", &q.client),
        list("API is", &q.api),
        list("topic is", &q.topic),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !q.text.is_empty() {
        out.push(format!("text contains \u{201c}{}\u{201d}", q.text));
    }
    if q.refused_only {
        out.push("it was refused".to_owned());
    }
    out
}

impl LiveResponse {
    #[must_use]
    pub fn new(live: LiveView) -> Self {
        let words = LiveWords {
            empty: format!(
                "No requests yet. Send one with any Kafka client, for example: echo hi | kcat -P -b {} -t orders -X client.id=admin-cli",
                live.listen
            ),
            phrases: phrases(&live.query),
            state: match live.state {
                LiveState::Quiet => Some("Nothing yet".to_owned()),
                LiveState::Busy => Some("A busy minute".to_owned()),
                LiveState::Steady => None,
            },
        };
        let tail_from = cursor(live.cursor);
        Self {
            live,
            words,
            tail_from,
        }
    }
}

impl RequestResponse {
    #[must_use]
    pub fn new(d: WireDetail, limits: RequestLimits, listen: &str) -> Self {
        let kind = RequestKind::of(&d);
        let rules = match kind {
            RequestKind::Detail => vec![],
            RequestKind::Refused => refused_rules(&d, &limits),
            RequestKind::Fenced => fenced_rules(&d),
            RequestKind::TooLarge => too_large_rules(&d, &limits),
        };
        let fetch = (d.row.api == "Fetch")
            .then(|| {
                let (t, parts) = d.topics.first()?;
                let (p, o) = (*parts.first()?, d.from_offset?);
                let c = fetch_commands(listen, t, p, o);
                Some(FetchCopy {
                    topic: t.clone(),
                    partition: p,
                    offset: o,
                    kcat: c.kcat,
                    console: c.console,
                })
            })
            .flatten();
        let compacted = limits
            .cleanup
            .as_deref()
            .is_some_and(|c| c.contains("compact"));
        let keyed = (d.row.code == 87 && compacted && d.row.class != WireClass::Denied)
            .then(|| {
                d.topics
                    .first()
                    .map(|(t, _)| keyed_produce(listen, t, &d.row.client))
            })
            .flatten();
        Self {
            request: d,
            kind,
            limits,
            rules,
            fetch,
            keyed,
        }
    }
}

/// `GET /api/v1/live?q=&client=&api=&topic=&refused=1&idle=0&before=&limit=`.
pub async fn live(State(s): State<Shared>, RawQuery(query): RawQuery) -> Response {
    let f = fields(query.as_deref());
    let q = query_of(&f);
    let view = s.source.live(&q, crate::form::get(&f, "state")).await;
    json(StatusCode::OK, &LiveResponse::new(view))
}

/// `GET /api/v1/live/request/{id}`.
pub async fn request(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    let found = match id.parse::<u64>() {
        Ok(id) => s.source.request(id).await,
        Err(_) => None,
    };
    let found = found.map(|(d, limits)| RequestResponse::new(d, limits, &s.source.listen()));
    let gone = found.is_none().then(|| GONE.to_owned());
    json(StatusCode::OK, &RequestAnswer { found, gone })
}

/// `GET /api/v1/live/clients`.
pub async fn clients(State(s): State<Shared>) -> Response {
    json::<ClientsView>(StatusCode::OK, &s.source.clients().await)
}

/// `GET /api/v1/live/first`.
pub async fn first(State(s): State<Shared>) -> Response {
    json(
        StatusCode::OK,
        &FirstResponse {
            first: s.source.first().await,
        },
    )
}

/// `GET /api/v1/live/topic-wire?topic=`.
pub async fn topic_wire(State(s): State<Shared>, RawQuery(query): RawQuery) -> Response {
    let f = fields(query.as_deref());
    let topic = crate::form::text(&f, "topic").trim().to_owned();
    let (rows, window, now_ms) = s.source.topic_wire(&topic).await;
    json(
        StatusCode::OK,
        &TopicWireView {
            topic,
            rows,
            window,
            now_ms,
        },
    )
}

/// `GET /api/v1/live/group-wire?group=&kind=&refused=1&q=`.
pub async fn group_wire(State(s): State<Shared>, RawQuery(query): RawQuery) -> Response {
    let f = fields(query.as_deref());
    let q = WireQuery {
        group: crate::form::text(&f, "group").trim().to_owned(),
        ..WireQuery::parse(&f)
    };
    json::<GroupWireView>(StatusCode::OK, &s.source.group_wire(&q).await)
}

/// What a tail follows, and where it is.
struct Tail {
    q: WireQuery,
    version: u64,
}

impl Tail {
    /// One look: the rows changed since the last, or `None` when nothing was.
    async fn look(&mut self, s: &Shared) -> Option<TailBatch> {
        let (next, rows) = s.source.tail_since(&self.q, self.version).await;
        self.version = next;
        (!rows.is_empty()).then(|| TailBatch {
            cursor: cursor(next),
            rows,
        })
    }
}

fn data<T: Serialize>(name: &str, value: &T, id: Option<&str>) -> Event {
    let e = Event::default()
        .event(name)
        .data(serde_json::to_string(value).unwrap_or_else(|_| "{}".to_owned()));
    match id {
        Some(id) => e.id(id),
        None => e,
    }
}

/// The events of one tail: `ready` at once, then one `rows` per look that found something, until the console stops.
fn events(
    s: Shared,
    tail: Tail,
    reset: bool,
    pace: Duration,
    slot: tokio::sync::OwnedSemaphorePermit,
) -> impl Stream<Item = Result<Event, Infallible>> {
    let ready = TailReady {
        cursor: cursor(tail.version),
        reset,
        pace: pace.as_secs(),
    };
    let first = stream::once(std::future::ready(Ok(data("ready", &ready, None))));
    let mut every = tokio::time::interval(pace);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // The permit lives as long as the stream does. The first tick of an interval is at once: the look a person who just
    // switched the tail on (or came back after a drop) wants.
    let rest = stream::unfold(
        (s, tail, every, slot),
        |(s, mut tail, mut every, slot)| async move {
            loop {
                tokio::select! {
                    () = s.stop.cancelled() => return None,
                    _ = every.tick() => {}
                }
                if let Some(batch) = tail.look(&s).await {
                    let e = data("rows", &batch, Some(&batch.cursor));
                    return Some((Ok(e), (s, tail, every, slot)));
                }
            }
        },
    );
    first.chain(rest)
}

/// `GET /api/v1/tail/live?since=<cursor>&pace=<seconds>&<filters>`, `Last-Event-ID` winning over `since`.
pub async fn tail(
    State(s): State<Shared>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let f = fields(query.as_deref());
    let asked = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .or_else(|| crate::form::get(&f, "since").map(str::to_owned));
    let q = WireQuery {
        before: None,
        ..WireQuery::parse(&f)
    };
    let (now, _) = s.source.tail_since(&q, u64::MAX).await;
    let (version, reset) = match asked.as_deref().map(version_of) {
        // A cursor of this life that the recorder has not passed: from there.
        Some(Some(v)) if v <= now => (v, false),
        // No cursor: from now.
        None => (now, false),
        // A cursor of another life (the broker restarted), or one ahead of the recorder: the client drops its tape and gets
        // the rows of this life.
        _ => (0, true),
    };
    let Ok(permit) = Arc::clone(&s.streams).try_acquire_owned() else {
        return super::busy(
            "The console has as many event streams open as it allows. Close a tail in another tab and switch this one on again.",
        );
    };
    let pace = crate::stream::pace_of(crate::form::get(&f, "pace"));
    Sse::new(events(s, Tail { q, version }, reset, pace, permit))
        .keep_alive(
            KeepAlive::new()
                .interval(crate::stream::KEEP_ALIVE)
                .text("keep-alive"),
        )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cursor_of_another_life_names_no_version() {
        assert_eq!(version_of(&cursor(42)), Some(42));
        assert_eq!(version_of("1.42"), None);
        assert_eq!(version_of("42"), None);
        assert_eq!(version_of(""), None);
        assert_eq!(version_of(&format!("{}.x", epoch())), None);
    }

    #[test]
    fn the_snapshot_asks_for_at_most_a_hundred_rows() {
        let f = |s: &str| crate::form::parse(s);
        assert_eq!(query_of(&f("")).limit, SNAPSHOT_MOST);
        assert_eq!(query_of(&f("limit=7")).limit, 7);
        assert_eq!(query_of(&f("limit=5000")).limit, SNAPSHOT_MOST);
        assert_eq!(
            phrases(&query_of(&f("client=a&client=b&refused=1"))),
            ["app is a or b", "it was refused"]
        );
    }
}
