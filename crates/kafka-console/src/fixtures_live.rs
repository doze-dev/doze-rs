//! The "larder" world of the Live boards as view-models: the 16 rows of `live-wire`, the paused, quiet and busy states, the five
//! drawers, the clients, the first request, and the two docks. Used by the tests and the `boards` example; the headline numbers
//! are checked against the boards' text by `tests/boards_live.rs`.

#![allow(clippy::too_many_lines, clippy::too_many_arguments)] // fixtures are tables of the boards' rows

use crate::fixtures_topics::now as world_now;
use crate::model::{
    LiveState, LiveView, WireClass, WireDetail, WireQuery, WireRefusal, WireRow, WireWindow,
};
use crate::pages::live::RequestLimits;
use crate::recorder::errors;

mod others;
mod rows;

pub use others::{clients, first, group_wire, spoke, topic_wire};

pub use rows::{busy_rows, steady_rows};

/// The moment of the world: Nov 9, 22:14:25 UTC.
#[must_use]
pub fn now() -> i64 {
    world_now()
}

/// The start of the day of the world.
fn day() -> i64 {
    now() - (22 * 3600 + 14 * 60 + 25) * 1000
}

/// A time of the day (`22:14:22.904`) as milliseconds since the epoch.
pub(crate) fn at(t: &str) -> i64 {
    let mut parts = t.split([':', '.']).map(|p| p.parse::<i64>().unwrap_or(0));
    let (h, m, s, ms) = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );
    day() + ((h * 60 + m) * 60 + s) * 1000 + ms
}

/// Fields of a fixture row that the world's rows differ in.
pub(crate) struct Spec<'a> {
    pub id: u64,
    pub time: &'a str,
    pub client: &'a str,
    pub tag: &'a str,
    pub api: &'static str,
    pub version: i16,
    pub target: &'a str,
    pub sub: Option<&'a str>,
    pub class: WireClass,
    pub code: i16,
    pub detail: &'a str,
    pub took_ms: u32,
}

/// A row from its spec.
pub(crate) fn wr(s: &Spec<'_>) -> WireRow {
    let topic = s.target.rsplit_once('-').map_or(s.target, |(t, p)| {
        if p.chars().all(|c| c.is_ascii_digit()) {
            t
        } else {
            s.target
        }
    });
    WireRow {
        id: s.id,
        at_ms: at(s.time),
        since_ms: None,
        count: 1,
        client: s.client.to_owned(),
        tag: (!s.tag.is_empty()).then(|| s.tag.to_owned()),
        api: s.api,
        version: s.version,
        transactional: false,
        target: s.target.to_owned(),
        target_sub: s.sub.map(str::to_owned),
        topics: if matches!(
            s.api,
            "OffsetCommit" | "LeaveGroup" | "JoinGroup" | "Heartbeat" | "SyncGroup"
        ) {
            vec![]
        } else {
            vec![topic.to_owned()]
        },
        group: matches!(
            s.api,
            "OffsetCommit" | "LeaveGroup" | "JoinGroup" | "Heartbeat" | "SyncGroup"
        )
        .then(|| s.target.to_owned()),
        class: s.class,
        code: s.code,
        code_name: errors::name(s.code),
        detail: s.detail.to_owned(),
        took_us: s.took_ms * 1000,
        took_sub: None,
        idle: false,
    }
}

fn window(rows: usize, requests: u64) -> WireWindow {
    WireWindow {
        rows,
        capacity: 2000,
        oldest_ms: Some(at("19:12:00.000")),
        requests,
    }
}

fn refused_links(rows: &[WireRow]) -> Vec<WireRefusal> {
    let mut out: Vec<WireRefusal> = rows
        .iter()
        .filter(|r| r.class == WireClass::Refused)
        .map(|r| WireRefusal {
            id: r.id,
            count: 1,
            code_name: r.code_name.clone(),
            code: r.code,
            client: r.client.clone(),
            at_ms: r.at_ms,
            class: r.class,
        })
        .collect();
    out.push(WireRefusal {
        id: 99,
        count: 1,
        code_name: "INVALID_PRODUCER_EPOCH".into(),
        code: 47,
        client: "orders".into(),
        at_ms: at("19:31:08.417"),
        class: WireClass::Refused,
    });
    out
}

/// A poll that found nothing, the oldest row of the tape: "Idle polls hidden" hides it, and turning that off shows it.
fn idle_poll() -> WireRow {
    let mut r = steady_rows().remove(0);
    // It opens the drawer of a request the fixtures have (the world has no request of its own for it).
    r.id = 99;
    r.at_ms = at("22:14:21.000");
    r.target_sub = None;
    r.detail = "no records".to_owned();
    r.idle = true;
    r
}

/// Whether the query asks for anything but the tape as it is (the page's own state, a pause or a page back, is not a filter).
fn is_filtered(q: &WireQuery) -> bool {
    !(q.text.is_empty()
        && q.client.is_empty()
        && q.api.is_empty()
        && q.topic.is_empty()
        && q.group.is_empty()
        && !q.refused_only
        && q.hide_idle)
}

fn has(hay: &str, needle: &str) -> bool {
    hay.to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

/// Whether a row of the fixture tape passes the person's filters: the rules of the real recorder (`recorder/view.rs`), so that the
/// fixtures server answers a filter the way a broker with traffic does (it used to ignore the query, and no filter changed anything).
fn passes(r: &WireRow, q: &WireQuery) -> bool {
    let in_topics = |n: &str| r.topics.iter().any(|t| has(t, n));
    let in_group = |n: &str| r.group.as_deref().is_some_and(|g| has(g, n));
    q.client_ok(&r.client)
        && q.api_ok(r.api)
        && q.topic_ok(r.topics.iter().map(String::as_str))
        && (q.group.is_empty() || in_group(&q.group))
        && (q.text.is_empty()
            || has(&r.client, &q.text)
            || has(r.api, &q.text)
            || in_topics(&q.text)
            || in_group(&q.text))
        && (!q.refused_only || r.class == WireClass::Refused)
        && (!q.hide_idle || !r.idle)
        && q.before.is_none_or(|b| r.id < b)
}

/// The Live page in the state `hint` names: `quiet`, `load`; anything else is the steady tape.
#[must_use]
pub fn live(q: &WireQuery, hint: Option<&str>) -> LiveView {
    let steady = steady_rows();
    let mut v = LiveView {
        state: LiveState::Steady,
        rows: steady.clone(),
        matched: 2000,
        window: window(2000, 2000),
        query: q.clone(),
        refused: refused_links(&steady),
        refused_more: 0,
        refused_total: 3,
        denied_total: 0,
        clients: 9,
        last_minute: 14,
        busiest: Some(("clickstream".to_owned(), 412)),
        seen: 2000,
        newest_id: 16,
        cursor: 2000,
        now_ms: now(),
        listen: "127.0.0.1:9092".to_owned(),
        started_ms: Some(at("22:12:00.000")),
        older: Some(1),
        facets: others::facets(&steady),
    };
    match hint {
        Some("quiet") => {
            v.state = LiveState::Quiet;
            v.rows = vec![];
            v.matched = 0;
            v.clients = 0;
            v.refused = vec![];
            v.refused_total = 0;
            v.window = WireWindow {
                rows: 0,
                capacity: 2000,
                oldest_ms: None,
                requests: 0,
            };
            v.seen = 0;
            v.older = None;
        }
        Some("load") => {
            v.state = LiveState::Busy;
            v.rows = busy_rows();
            v.matched = 7;
            v.window = window(7, 571);
            v.last_minute = 571;
            v.refused = vec![];
            v.refused_total = 0;
            v.older = None;
        }
        _ if is_filtered(q) => {
            // The steady tape answers the filters like the real one: only the rows that pass (the polls that found nothing are
            // there too, hidden by default), and no pager unless more than a page passes.
            let mut all = steady.clone();
            all.insert(4, idle_poll());
            let kept: Vec<WireRow> = all.into_iter().filter(|r| passes(r, q)).collect();
            v.matched = kept.len();
            v.older = None;
            v.rows = kept.into_iter().take(q.limit).collect();
        }
        _ => {}
    }
    v
}

fn detail_of(row: WireRow, f: impl FnOnce(&mut WireDetail)) -> WireDetail {
    let (words, plain, pass) = errors::of(row.code).map_or_else(Default::default, |e| {
        (e.words.to_owned(), e.plain.to_owned(), e.pass.to_owned())
    });
    let mut d = WireDetail {
        principal: "User:ANONYMOUS".to_owned(),
        software: Some("librdkafka 2.3.0".to_owned()),
        peer: Some("127.0.0.1:50402".to_owned()),
        correlation: 9114,
        conn: 3,
        request_bytes: 120,
        response_bytes: 260,
        acks: None,
        wait_ms: None,
        isolation: None,
        from_offset: None,
        producer: None,
        txn_id: None,
        member: None,
        generation: None,
        reason: None,
        what: None,
        note: None,
        record_note: None,
        records: 0,
        batches: 0,
        bytes: 0,
        items: 0,
        parts: vec![],
        parts_more: 0,
        topics: row.topics.iter().map(|t| (t.clone(), vec![])).collect(),
        words,
        plain,
        pass,
        took_spread: None,
        story: vec![],
        row,
    };
    f(&mut d);
    d
}

/// A request in full and what the broker says of its topic now, for the drawer boards by id: `detail` (the Fetch), `refused`,
/// `fenced`, `too-large`.
#[must_use]
pub fn request(which: &str) -> Option<(WireDetail, RequestLimits)> {
    let rows = steady_rows();
    let find = |code: i16| rows.iter().find(|r| r.code == code).cloned();
    match which {
        "detail" => {
            let mut row = rows.first().cloned()?;
            row.took_sub = Some("willing to wait 480 ms".into());
            Some((
                detail_of(row, |d| {
                    d.topics = vec![("payments.settled".into(), vec![1])];
                    d.from_offset = Some(7402);
                    d.wait_ms = Some(480);
                    d.software = Some("confluent-kafka (librdkafka)".into());
                }),
                RequestLimits::default(),
            ))
        }
        "refused" => {
            Some((
                detail_of(find(87)?, |d| {
                    d.record_note = Some("Compacted topic cleanup.policy=compact requires every record to have a key".into());
                    d.topics = vec![("inventory.levels".into(), vec![0])];
                    d.records = 1;
                    d.software = Some("librdkafka".into());
                }),
                RequestLimits {
                    cleanup: Some("compact".into()),
                    max_message_bytes: None,
                },
            ))
        }
        "too-large" => Some((
            detail_of(find(10)?, |d| {
                d.request_bytes = 1_203_441;
                d.topics = vec![("orders".into(), vec![1])];
                d.software = Some("sarama".into());
            }),
            RequestLimits {
                cleanup: None,
                max_message_bytes: Some((1_048_588, false)),
            },
        )),
        "fenced" => Some((fenced(), RequestLimits::default())),
        _ => None,
    }
}

/// The drawer of the row with this id in the world's tape (any row of the steady tape; 99 is the fenced producer).
#[must_use]
pub fn request_by_id(id: u64) -> Option<(WireDetail, RequestLimits)> {
    if id == 99 {
        return request("fenced");
    }
    let row = steady_rows().into_iter().find(|r| r.id == id)?;
    request(match row.code {
        87 => "refused",
        10 => "too-large",
        _ if row.api == "Fetch" => "detail",
        _ => return Some((detail_of(row, |_| {}), RequestLimits::default())),
    })
}

fn fenced() -> WireDetail {
    let row = wr(&Spec {
        id: 99,
        time: "19:31:08.417",
        client: "orders",
        tag: "go",
        api: "Produce",
        version: 9,
        target: "orders-0",
        sub: None,
        class: WireClass::Refused,
        code: 47,
        detail: "",
        took_ms: 1,
    });
    let init = wr(&Spec {
        id: 40,
        time: "17:13:52.000",
        client: "orders",
        tag: "go",
        api: "InitProducerId",
        version: 4,
        target: "orders-eos",
        sub: None,
        class: WireClass::Done,
        code: 0,
        detail: "",
        took_ms: 3,
    });
    detail_of(row, |d| {
        d.row.transactional = true;
        d.topics = vec![("orders".into(), vec![0])];
        d.producer = Some((4017, 1));
        d.txn_id = Some("orders-eos".into());
        d.software = Some("sarama".into());
        d.story = vec![init];
    })
}
