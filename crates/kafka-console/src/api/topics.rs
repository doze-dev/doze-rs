//! The JSON API of the Topics pages (slice R1a, the read side): the list, a topic's frame (head, stats, tabs, the open
//! transaction), its Messages and one record, its Consumers, its Log (and the Lab's Compact now), its Producers and a
//! transaction, and the Messages tail (`/api/v1/tail/messages`, server-sent events, CON-809).
//!
//! Every answer is the view-model the classic pages render (`gather/`, `model/`), serialised, with the words Rust already
//! decides beside it. A name that is no topic is `UNKNOWN_TOPIC_OR_PARTITION`, 404, in the classic page's words.

use axum::Router;
use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::get;
use kafka_wire::ids::TopicName;
use serde::Serialize;

use super::{error, json};
use crate::form::{get as field, parse};
use crate::model::{
    Cleanup, ConsumersView, MessageQuery, MessagesView, RecordView, Span, TopicHead, TopicsState,
    TopicsView,
};
use crate::state::Shared;

/// `delete`, `compact`, `compact,delete`: the policy as the pill says it.
#[must_use]
pub fn policy_word(c: Cleanup) -> &'static str {
    match c {
        Cleanup::Delete => "delete",
        Cleanup::Compact => "compact",
        Cleanup::Both => "compact,delete",
    }
}

/// `GET /api/v1/topics?q=&internal=1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TopicsResponse {
    pub view: TopicsView,
    pub state: TopicsState,
}

/// Which buttons a topic's head carries: a function of the topic alone, so the same on every tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum TopicActs {
    /// Partitions, Produce, Delete topic.
    Full,
    /// Produce and Delete topic: nothing in it, no partitions page worth opening.
    Empty,
    /// Partitions and Produce: an offline partition.
    Offline,
}

/// `GET /api/v1/topics/{topic}`: what every page of a topic shows above its tabs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TopicFrame {
    pub head: TopicHead,
    /// `delete`, `compact`, `compact,delete`.
    pub policy: String,
    /// The Retention stat: `delete · 7 d`, or the policy alone.
    pub retention: String,
    pub records: i64,
    pub bytes: u64,
    pub last_write_ms: Option<i64>,
    /// Nothing was ever written.
    pub empty: bool,
    /// Records held back from `read_committed` consumers, over every partition.
    pub held: i64,
    /// The partitions whose writes are refused.
    pub offline: Vec<i32>,
    pub acts: TopicActs,
}

impl TopicFrame {
    #[must_use]
    pub fn new(head: TopicHead) -> Self {
        let policy = policy_word(head.cleanup).to_owned();
        let retention = head.retention_ms.map_or_else(
            || policy.clone(),
            |ms| format!("{policy} · {}", crate::words::retention(ms)),
        );
        let offline: Vec<i32> = head.offline().iter().map(|p| p.partition).collect();
        let acts = if !offline.is_empty() {
            TopicActs::Offline
        } else if head.is_empty() {
            TopicActs::Empty
        } else {
            TopicActs::Full
        };
        Self {
            policy,
            retention,
            records: head.records(),
            bytes: head.bytes(),
            last_write_ms: head.last_write_ms(),
            empty: head.is_empty(),
            held: head.held(),
            offline,
            acts,
            head,
        }
    }
}

/// `GET /api/v1/topics/{topic}/messages?partition=&offset=&time=&key=&isolation=&limit=`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct MessagesResponse {
    pub view: MessagesView,
    /// What the read did, as a sentence (only when a filter is set).
    pub reach: Vec<Span>,
    /// The Live tail applies: newest records, in the order they arrive (no key, time or offset).
    pub tailable: bool,
    /// The newest timestamp on the page: where a tail starts.
    pub cursor: i64,
    /// "Older" is offered: more matched and the page is under the most it shows; it asks for this limit.
    pub older: Option<usize>,
}

/// One row of the Consumers tab: a partition of this topic as one group stands on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ReaderPartition {
    pub partition: i32,
    pub end: Option<i64>,
    pub committed: Option<i64>,
    /// `behind`, `caught_up`, `past_end`, `no_commit`, `gone`.
    pub lag: String,
    /// The records behind, or past the end.
    pub by: i64,
    /// Per cent of the bar.
    pub bar: u8,
}

/// A group that reads the topic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Reader {
    pub id: String,
    /// The consumer protocol (KIP-848), else classic.
    pub consumer_protocol: bool,
    /// `Stable`, `Empty`, `Rebalancing`, `Assigning`, `Reconciling`.
    pub phase: String,
    pub members: usize,
    pub holding: usize,
    pub behind: i64,
    pub last_commit_ms: Option<i64>,
    pub rows: Vec<ReaderPartition>,
}

/// `GET /api/v1/topics/{topic}/consumers`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConsumersResponse {
    pub topic: String,
    pub readers: Vec<Reader>,
    /// How many groups have committed on the topic, before the cap of what is listed.
    pub total: usize,
    pub now_ms: i64,
}

impl ConsumersResponse {
    #[must_use]
    pub fn new(v: ConsumersView) -> Self {
        use crate::lag::Lag;
        use crate::model::GroupProtocol;
        let readers = v
            .readers
            .into_iter()
            .map(|r| Reader {
                consumer_protocol: r.protocol == GroupProtocol::Consumer,
                phase: r.phase.word().to_owned(),
                members: r.members,
                holding: r.holding,
                behind: r.block.behind,
                last_commit_ms: r.last_commit_ms,
                rows: r
                    .block
                    .rows
                    .iter()
                    .map(|p| {
                        let (lag, by) = match p.lag {
                            Lag::Behind(n) => ("behind", n),
                            Lag::CaughtUp => ("caught_up", 0),
                            Lag::PastEnd(n) => ("past_end", n),
                            Lag::NoCommit => ("no_commit", 0),
                            Lag::Gone => ("gone", 0),
                        };
                        ReaderPartition {
                            partition: p.partition,
                            end: p.end,
                            committed: p.committed,
                            lag: lag.to_owned(),
                            by,
                            bar: p.bar,
                        }
                    })
                    .collect(),
                id: r.id,
            })
            .collect();
        Self {
            topic: v.head.name,
            readers,
            total: v.total,
            now_ms: v.head.now_ms,
        }
    }
}

pub(super) fn valid(name: &str) -> bool {
    TopicName::parse(name).is_ok()
}

/// The answer of a page's lookup that found nothing (a topic, a record, a transaction): `{"missing": {code, message}}`, 200.
/// A page asks for the thing to know whether it is there; "it is not" is an answer, not a failure, and a 404 would put an
/// error in the browser's console (Chromium and Safari log every failed fetch) on a page that is working as it should.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Missing {
    pub missing: super::ApiError,
}

/// [`Missing`] as a response.
pub(super) fn missing(code: &str, message: &str) -> Response {
    json(
        StatusCode::OK,
        &Missing {
            missing: super::ApiError {
                code: code.to_owned(),
                message: message.to_owned(),
            },
        },
    )
}

/// No such topic, as a lookup's answer (the frame).
fn no_such_topic(name: &str) -> Response {
    let shown: String = name.chars().take(80).collect();
    missing(
        "UNKNOWN_TOPIC_OR_PARTITION",
        &format!("There is no topic called {shown}, or the console may not describe it."),
    )
}

pub(super) fn no_topic(name: &str) -> Response {
    let shown: String = name.chars().take(80).collect();
    error(
        StatusCode::NOT_FOUND,
        "UNKNOWN_TOPIC_OR_PARTITION",
        &format!("There is no topic called {shown}, or the console may not describe it."),
    )
}

pub(super) fn fields(q: Option<&str>) -> Vec<(String, String)> {
    parse(q.unwrap_or(""))
}

/// `?partition=`: a non-negative whole number, else the first.
pub(super) fn partition_of(f: &[(String, String)]) -> i32 {
    field(f, "partition")
        .and_then(|p| p.trim().parse::<i32>().ok())
        .filter(|p| *p >= 0)
        .unwrap_or(0)
}

async fn topics(State(s): State<Shared>, RawQuery(q): RawQuery) -> Response {
    let f = fields(q.as_deref());
    let filter: String = field(&f, "q")
        .unwrap_or("")
        .trim()
        .chars()
        .take(249)
        .collect();
    let internal = field(&f, "internal") == Some("1");
    let view = s.source.topics(&filter, internal, None).await;
    let state = view.state();
    json(StatusCode::OK, &TopicsResponse { view, state })
}

async fn frame(State(s): State<Shared>, Path(name): Path<String>) -> Response {
    if !valid(&name) {
        return no_such_topic(&name);
    }
    match s.source.topic_head(&name).await {
        Some(h) => json(StatusCode::OK, &TopicFrame::new(h)),
        None => no_such_topic(&name),
    }
}

async fn messages(
    State(s): State<Shared>,
    Path(name): Path<String>,
    RawQuery(q): RawQuery,
) -> Response {
    if !valid(&name) {
        return no_topic(&name);
    }
    let (query, notes) = MessageQuery::parse(&fields(q.as_deref()));
    let Some(view) = s.source.messages(&name, query, notes).await else {
        return no_topic(&name);
    };
    let tailable =
        view.query.key.is_none() && view.query.offset.is_none() && view.query.time.is_none();
    let reach = if view.query.filtered() {
        crate::words::reach_sentence(&view)
    } else {
        Vec::new()
    };
    let older = (view.reach.more && view.query.limit < crate::model::MOST_LIMIT)
        .then(|| (view.query.limit * 2).min(crate::model::MOST_LIMIT));
    let cursor = view.rows.iter().map(|r| r.timestamp_ms).max().unwrap_or(0);
    json(
        StatusCode::OK,
        &MessagesResponse {
            view,
            reach,
            tailable,
            cursor,
            older,
        },
    )
}

async fn record(
    State(s): State<Shared>,
    Path((name, partition, offset)): Path<(String, String, String)>,
) -> Response {
    if !valid(&name) {
        return no_topic(&name);
    }
    let (Some(p), Some(o)) = (
        partition.parse::<i32>().ok().filter(|p| *p >= 0),
        offset.parse::<i64>().ok().filter(|o| *o >= 0),
    ) else {
        return missing(
            "NO_SUCH_RECORD",
            "A record's address is a partition and an offset, both whole numbers from 0.",
        );
    };
    match s.source.record(&name, p, o).await {
        Some(r) => json::<RecordView>(StatusCode::OK, &r),
        None => missing(
            "NO_SUCH_RECORD",
            &format!(
                "There is no record at offset {} of {name}-{p}. The offset may be past the end of the log or before its start, or the record may have been removed by compaction, by a deletion, or be a transaction marker.",
                crate::format::number(o)
            ),
        ),
    }
}

async fn consumers(State(s): State<Shared>, Path(name): Path<String>) -> Response {
    if !valid(&name) {
        return no_topic(&name);
    }
    match s.source.consumers(&name).await {
        Some(v) => json(StatusCode::OK, &ConsumersResponse::new(v)),
        None => no_topic(&name),
    }
}

/// Whether the Lab is on: a live broker started with it.
mod log;
mod tail;
#[cfg(test)]
pub(super) mod types;

pub use log::{
    CompactDialog, CompactDone, LogResponse, LogToCome, ProducersResponse, TxnResponse,
    log_response,
};
pub use tail::{TAIL_WINDOW, TailRows};

/// The routes of this module, joined to the API's (`api::routes`), behind the same guard.
pub fn routes(web: &crate::base::Base) -> Router<Shared> {
    Router::new()
        .route(&web.url("/api/v1/topics"), get(topics))
        .route(&web.url("/api/v1/topics/{topic}"), get(frame))
        .route(&web.url("/api/v1/topics/{topic}/messages"), get(messages))
        .route(
            &web.url("/api/v1/topics/{topic}/records/{partition}/{offset}"),
            get(record),
        )
        .route(&web.url("/api/v1/topics/{topic}/consumers"), get(consumers))
        .route(&web.url("/api/v1/topics/{topic}/log"), get(log::log))
        .route(
            &web.url("/api/v1/topics/{topic}/log/compact"),
            axum::routing::post(log::compact),
        )
        .route(
            &web.url("/api/v1/topics/{topic}/producers"),
            get(log::producers),
        )
        .route(
            &web.url("/api/v1/topics/{topic}/transactions/{id}"),
            get(log::txn),
        )
        .route(&web.url("/api/v1/tail/messages"), get(tail::tail_messages))
}
