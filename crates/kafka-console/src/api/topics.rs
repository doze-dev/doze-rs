//! The JSON API of the Topics pages (slice R1a, the read side): the list, a topic's frame (head, stats, tabs, the open
//! transaction), its Messages and one record, its Consumers, its Log (and the Lab's Compact now), its Producers and a
//! transaction, and the Messages tail (`/api/v1/tail/messages`, server-sent events, CON-809).
//!
//! Every answer is the view-model the classic pages render (`gather/`, `model/`), serialised, with the words Rust already
//! decides beside it. A name that is no topic is `UNKNOWN_TOPIC_OR_PARTITION`, 404, in the classic page's words.

use std::collections::HashSet;
use std::convert::Infallible;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::stream::{self, StreamExt};
use kafka_wire::ids::TopicName;
use serde::Serialize;

use super::{error, json};
use crate::form::{get as field, parse};
use crate::log_ruler::Ruler;
use crate::log_words::{Expiry, Rule};
use crate::model::{
    Cleanup, ConsumersView, LogState, LogView, MessageQuery, MessagesView, ProducersView,
    RecordRow, RecordView, Span, TopicHead, TopicsState, TopicsView, TxnView,
};
use crate::source::Source;
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

/// What a log that is not due still needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogToCome {
    /// `dirty over 15.2 MiB`'s figure.
    pub needs: String,
    pub has: String,
    pub coming: Vec<Span>,
}

/// The Compact now dialog: the planner's verdict and what a round would do, or why nothing can be done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CompactDialog {
    /// `61 % dirty against 50 %`, when a round is due.
    pub due: Option<String>,
    pub rewrite: Vec<Span>,
    /// Where the checkpoint is and where the round would move it.
    pub checkpoint: (i64, i64),
    /// Why nothing can be done, in a sentence.
    pub blocked: Option<String>,
    /// The Lab is switched off (`--lab false`).
    pub off: bool,
}

/// `GET /api/v1/topics/{topic}/log?partition=`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogResponse {
    pub view: LogView,
    pub state: LogState,
    pub ruler: Ruler,
    /// The planner's rules with this partition's numbers (a log that compacts).
    pub rules: Vec<Rule>,
    pub would_rewrite: Vec<Span>,
    /// `Nov 9 21:31, 43 min ago, on inventory.levels-0.`
    pub round: Option<String>,
    pub to_come: Option<LogToCome>,
    /// Retention's rules (a log that deletes).
    pub retention_rules: Vec<Rule>,
    pub expiry: Option<Expiry>,
    pub compact: CompactDialog,
}

/// `POST /api/v1/topics/{topic}/log/compact?partition=`: what the round did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CompactDone {
    pub message: String,
}

/// `GET /api/v1/topics/{topic}/producers?partition=`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProducersResponse {
    pub view: ProducersView,
    pub held: i64,
}

/// `GET /api/v1/topics/{topic}/transactions/{id}`: the transaction, with the sentences of its two pages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TxnResponse {
    pub view: TxnView,
    pub held: i64,
    pub sentence: Vec<Span>,
    pub holds: Vec<Span>,
    /// What happens if it is not ended: before and after the refusal a fenced producer meets.
    pub unended_before: Vec<Span>,
    pub unended_after: Vec<Span>,
    /// The sentence of the page that says who is held.
    pub holding: Vec<Span>,
    pub running_ms: i64,
    pub left_ms: i64,
    pub timeout_percent: i64,
    pub timeout: String,
}

fn valid(name: &str) -> bool {
    TopicName::parse(name).is_ok()
}

fn no_topic(name: &str) -> Response {
    let shown: String = name.chars().take(80).collect();
    error(
        StatusCode::NOT_FOUND,
        "UNKNOWN_TOPIC_OR_PARTITION",
        &format!("There is no topic called {shown}, or the console may not describe it."),
    )
}

fn fields(q: Option<&str>) -> Vec<(String, String)> {
    parse(q.unwrap_or(""))
}

/// `?partition=`: a non-negative whole number, else the first.
fn partition_of(f: &[(String, String)]) -> i32 {
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
        return no_topic(&name);
    }
    match s.source.topic_head(&name).await {
        Some(h) => json(StatusCode::OK, &TopicFrame::new(h)),
        None => no_topic(&name),
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
    let tailable = view.query.key.is_none() && view.query.offset.is_none() && view.query.time.is_none();
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
        return error(
            StatusCode::NOT_FOUND,
            "NO_SUCH_RECORD",
            "A record's address is a partition and an offset, both whole numbers from 0.",
        );
    };
    match s.source.record(&name, p, o).await {
        Some(r) => json::<RecordView>(StatusCode::OK, &r),
        None => error(
            StatusCode::NOT_FOUND,
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
fn lab_on(s: &Shared) -> bool {
    match &s.source {
        Source::Live(l) => l.lab.is_some(),
        #[cfg(any(test, feature = "examples"))]
        Source::Fixture => false,
    }
}

fn compact_dialog(v: &LogView, lab: bool) -> CompactDialog {
    let d = crate::lab::compact::dialog(v, lab);
    CompactDialog {
        due: d.due,
        rewrite: d.rewrite,
        checkpoint: d.checkpoint,
        blocked: d.blocked,
        off: d.off,
    }
}

/// The Log answer of a partition's log.
#[must_use]
pub fn log_response(v: LogView, lab: bool) -> LogResponse {
    let state = v.state();
    let ruler = crate::log_ruler::ruler(&v);
    let (rules, would_rewrite, round, to_come) = match &v.compaction {
        Some(c) => {
            let (needs, has, coming) = crate::log_words::still_to_come(&v, c);
            (
                crate::log_words::rules(&v),
                if c.planner.due {
                    crate::log_words::would_rewrite(&v)
                } else {
                    Vec::new()
                },
                crate::log_words::round_sentence(&v, c),
                (!c.planner.due).then_some(LogToCome { needs, has, coming }),
            )
        }
        None => (Vec::new(), Vec::new(), None, None),
    };
    let (retention_rules, expiry) = if v.retention.is_some() && v.compaction.is_none() {
        (
            crate::log_words::retention_rules(&v),
            crate::log_words::expiry(&v),
        )
    } else {
        (Vec::new(), None)
    };
    let compact = compact_dialog(&v, lab);
    LogResponse {
        state,
        ruler,
        rules,
        would_rewrite,
        round,
        to_come,
        retention_rules,
        expiry,
        compact,
        view: v,
    }
}

async fn log(State(s): State<Shared>, Path(name): Path<String>, RawQuery(q): RawQuery) -> Response {
    if !valid(&name) {
        return no_topic(&name);
    }
    let partition = partition_of(&fields(q.as_deref()));
    match s.source.log(&name, partition).await {
        Some(v) => json(StatusCode::OK, &log_response(v, lab_on(&s))),
        None => no_topic(&name),
    }
}

/// `POST /api/v1/topics/{topic}/log/compact?partition=`: the cleaner's real round, now (the Lab's Compact now). Refused,
/// with the dialog's sentence, when the topic does not compact, nothing is due or the Lab is off.
async fn compact(
    State(s): State<Shared>,
    Path(name): Path<String>,
    RawQuery(q): RawQuery,
) -> Response {
    if !valid(&name) {
        return no_topic(&name);
    }
    let partition = partition_of(&fields(q.as_deref()));
    let live = match &s.source {
        Source::Live(l) => l,
        #[cfg(any(test, feature = "examples"))]
        Source::Fixture => {
            return error(
                StatusCode::CONFLICT,
                "LAB_OFF",
                "The Lab runs on a live broker only.",
            );
        }
    };
    let Some(lab) = &live.lab else {
        return error(
            StatusCode::CONFLICT,
            "LAB_OFF",
            "The Lab is switched off (--lab false), so the console does not run the cleaner.",
        );
    };
    match crate::gather::lab::compact(lab, &live.backend, &name, partition).await {
        Ok(()) => {
            let message = match s.source.log(&name, partition).await.and_then(|v| v.compaction.and_then(|c| c.last)) {
                Some(r) => format!(
                    "Ran the cleaner on {name}-{partition}: {} segment{} replaced, checkpoint at {}.",
                    r.replaced,
                    if r.replaced == 1 { "" } else { "s" },
                    crate::format::number(r.clean_to)
                ),
                None => format!("Ran the cleaner on {name}-{partition}."),
            };
            json(StatusCode::OK, &CompactDone { message })
        }
        Err(v) => error(
            StatusCode::CONFLICT,
            "COMPACT_REFUSED",
            v.refused.as_deref().unwrap_or("The round was refused."),
        ),
    }
}

async fn producers(
    State(s): State<Shared>,
    Path(name): Path<String>,
    RawQuery(q): RawQuery,
) -> Response {
    if !valid(&name) {
        return no_topic(&name);
    }
    let partition = partition_of(&fields(q.as_deref()));
    match s.source.producers(&name, partition, Vec::new()).await {
        Some(view) => {
            let held = view.held();
            json(StatusCode::OK, &ProducersResponse { view, held })
        }
        None => no_topic(&name),
    }
}

async fn txn(
    State(s): State<Shared>,
    Path((name, id)): Path<(String, String)>,
) -> Response {
    use crate::producers_words as w;
    if !valid(&name) {
        return no_topic(&name);
    }
    let Some(view) = s.source.txn(&name, &id).await else {
        let shown: String = id.chars().take(80).collect();
        return error(
            StatusCode::NOT_FOUND,
            "TRANSACTIONAL_ID_NOT_FOUND",
            &format!("The broker knows no transaction called {shown}, or the console may not describe it. A transactional id is forgotten after 7 days idle."),
        );
    };
    let (unended_before, unended_after) = w::unended_parts(&view);
    let x = &view.txn;
    json(
        StatusCode::OK,
        &TxnResponse {
            held: view.held(),
            sentence: w::txn_sentence(&view),
            holds: w::holds_sentence(&view),
            unended_before,
            unended_after,
            holding: w::holding_sentence(&view),
            running_ms: w::running_ms(x, view.now_ms),
            left_ms: w::left_ms(x, view.now_ms),
            timeout_percent: w::timeout_percent(x, view.now_ms),
            timeout: w::timeout_text(x.timeout_ms),
            view,
        },
    )
}

/// The most records a Messages tail looks at in one look.
pub const TAIL_WINDOW: usize = crate::stream::MESSAGES_WINDOW;

/// One event of the Messages tail: the rows a look found that the page did not have, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TailRows {
    pub rows: Vec<RecordRow>,
}

/// What a tail follows: a topic's newest records, from a timestamp, with the keys of the last look.
struct Follow {
    topic: String,
    q: MessageQuery,
    since_ms: i64,
    last: Option<HashSet<(i32, i64)>>,
}

impl Follow {
    /// One look: the rows that are new since the last one (`None`: nothing new, nothing is sent).
    async fn look(&mut self, s: &Shared) -> Option<(i64, Vec<RecordRow>)> {
        let view = s.source.messages(&self.topic, self.q.clone(), vec![]).await?;
        let keys: HashSet<(i32, i64)> = view.rows.iter().map(|r| (r.partition, r.offset)).collect();
        let since = self.since_ms;
        let fresh: Vec<RecordRow> = view
            .rows
            .into_iter()
            .filter(|r| match &self.last {
                Some(prev) => !prev.contains(&(r.partition, r.offset)),
                None => r.timestamp_ms >= since,
            })
            .collect();
        self.last = Some(keys);
        let newest = fresh.iter().map(|r| r.timestamp_ms).max()?;
        self.since_ms = self.since_ms.max(newest);
        Some((self.since_ms, fresh))
    }
}

/// `GET /api/v1/tail/messages?topic=&pace=&since=&partition=&isolation=`: the Messages tail (CON-809), server-paced
/// (`pace` seconds between looks, 1 to 30) and coalesced: an event only when a look found new rows, its `id` the cursor
/// to resume from (`Last-Event-ID` on a reconnect, else `since`), so a reconnect brings what it missed and nothing twice.
async fn tail_messages(
    State(s): State<Shared>,
    RawQuery(q): RawQuery,
    headers: HeaderMap,
) -> Response {
    let f = fields(q.as_deref());
    let topic = crate::form::text(&f, "topic").to_owned();
    if !valid(&topic) {
        return no_topic(&topic);
    }
    let (mut query, _notes) = MessageQuery::parse(&f);
    if query.key.is_some() || query.offset.is_some() || query.time.is_some() {
        return error(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "A key, an offset or a time asks for a fixed read: only the newest records can be tailed.",
        );
    }
    query.limit = TAIL_WINDOW;
    if s.source.topic_head(&topic).await.is_none() {
        return no_topic(&topic);
    }
    let since = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .or_else(|| field(&f, "since"))
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(0);
    let Ok(permit) = Arc::clone(&s.streams).try_acquire_owned() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "BUSY",
            "The console has as many event streams open as it allows.",
        );
    };
    let pace = crate::stream::pace_of(field(&f, "pace"));
    let follow = Follow {
        topic,
        q: query,
        since_ms: since,
        last: None,
    };
    let ready = stream::once(std::future::ready(Ok::<Event, Infallible>(
        Event::default().event("ready").data("ok"),
    )));
    let mut every = tokio::time::interval(pace);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let rest = stream::unfold(
        (s, follow, every, permit),
        |(s, mut follow, mut every, permit)| async move {
            loop {
                tokio::select! {
                    () = s.stop.cancelled() => return None,
                    _ = every.tick() => {}
                }
                if let Some((id, rows)) = follow.look(&s).await {
                    let data = serde_json::to_string(&TailRows { rows }).unwrap_or_default();
                    let ev = Event::default().event("rows").id(id.to_string()).data(data);
                    return Some((Ok(ev), (s, follow, every, permit)));
                }
            }
        },
    );
    Sse::new(ready.chain(rest))
        .keep_alive(KeepAlive::new().interval(crate::stream::KEEP_ALIVE).text("keep-alive"))
        .into_response()
}

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
        .route(&web.url("/api/v1/topics/{topic}/log"), get(log))
        .route(
            &web.url("/api/v1/topics/{topic}/log/compact"),
            axum::routing::post(compact),
        )
        .route(&web.url("/api/v1/topics/{topic}/producers"), get(producers))
        .route(
            &web.url("/api/v1/topics/{topic}/transactions/{id}"),
            get(txn),
        )
        .route(&web.url("/api/v1/tail/messages"), get(tail_messages))
}

/// The TypeScript types of this module's answers, for `api::types`.
#[cfg(test)]
pub(super) mod types {
    use std::collections::BTreeMap;

    use super::super::types::one;
    use super::*;
    use crate::log_ruler::{Cell, Key, Mark, RulerCell};
    use crate::log_words::RuleTone;
    use crate::model::{
        CallVersions, HeaderRow, Hold, Kind, LogCleaner, LogCompaction, LogDurability, LogLimit,
        LogMoved, LogOther, LogOtherLink, LogPass, LogPlanner, LogRetentionView, LogRound,
        LogRules, LogSegment, LogStop, LogTombstone, OpenTxn, PartitionRow, ProdAborted,
        ProdBatch, ProdCell, ProdRow, ProdTail, ProdTxn, Reach, Reading, RecordTxn, Shown,
        Standing, TabCounts, TopicRow, TxnPart, TxnRow,
    };
    use ts_rs::Config;

    pub(crate) fn all(cfg: &Config, out: &mut BTreeMap<String, String>) {
        one::<TopicsResponse>(cfg, out);
        one::<TopicActs>(cfg, out);
        one::<TopicFrame>(cfg, out);
        one::<MessagesResponse>(cfg, out);
        one::<ReaderPartition>(cfg, out);
        one::<Reader>(cfg, out);
        one::<ConsumersResponse>(cfg, out);
        one::<LogToCome>(cfg, out);
        one::<CompactDialog>(cfg, out);
        one::<LogResponse>(cfg, out);
        one::<CompactDone>(cfg, out);
        one::<ProducersResponse>(cfg, out);
        one::<TxnResponse>(cfg, out);
        one::<TailRows>(cfg, out);
        one::<Cleanup>(cfg, out);
        one::<TopicRow>(cfg, out);
        one::<TopicsState>(cfg, out);
        one::<TopicsView>(cfg, out);
        one::<PartitionRow>(cfg, out);
        one::<OpenTxn>(cfg, out);
        one::<TabCounts>(cfg, out);
        one::<CallVersions>(cfg, out);
        one::<TopicHead>(cfg, out);
        one::<Standing>(cfg, out);
        one::<Kind>(cfg, out);
        one::<Shown>(cfg, out);
        one::<RecordRow>(cfg, out);
        one::<Hold>(cfg, out);
        one::<Reach>(cfg, out);
        one::<MessagesView>(cfg, out);
        one::<HeaderRow>(cfg, out);
        one::<RecordTxn>(cfg, out);
        one::<RecordView>(cfg, out);
        one::<Reading>(cfg, out);
        one::<MessageQuery>(cfg, out);
        one::<LogSegment>(cfg, out);
        one::<LogTombstone>(cfg, out);
        one::<LogLimit>(cfg, out);
        one::<LogCleaner>(cfg, out);
        one::<LogRound>(cfg, out);
        one::<LogRules>(cfg, out);
        one::<LogPlanner>(cfg, out);
        one::<LogCompaction>(cfg, out);
        one::<LogMoved>(cfg, out);
        one::<LogPass>(cfg, out);
        one::<LogRetentionView>(cfg, out);
        one::<LogStop>(cfg, out);
        one::<LogDurability>(cfg, out);
        one::<LogOtherLink>(cfg, out);
        one::<LogOther>(cfg, out);
        one::<LogState>(cfg, out);
        one::<LogView>(cfg, out);
        one::<ProdBatch>(cfg, out);
        one::<ProdTxn>(cfg, out);
        one::<ProdRow>(cfg, out);
        one::<ProdAborted>(cfg, out);
        one::<ProdCell>(cfg, out);
        one::<ProdTail>(cfg, out);
        one::<ProducersView>(cfg, out);
        one::<TxnRow>(cfg, out);
        one::<TxnView>(cfg, out);
        one::<TxnPart>(cfg, out);
        one::<Ruler>(cfg, out);
        one::<Mark>(cfg, out);
        one::<RulerCell>(cfg, out);
        one::<Cell>(cfg, out);
        one::<Key>(cfg, out);
        one::<Expiry>(cfg, out);
        one::<Rule>(cfg, out);
        one::<RuleTone>(cfg, out);
    }
}
