//! The "larder" world of the Live boards as view-models: the 16 rows of `live-wire`, the paused, quiet and busy states, the five
//! drawers, the clients, the first request, and the two docks. Used by the tests and the `boards` example; the headline numbers
//! are checked against the boards' text by `tests/boards_live.rs`.

#![allow(clippy::too_many_lines, clippy::too_many_arguments)] // fixtures are tables of the boards' rows

use crate::fixtures_topics::now as world_now;
use crate::model::{
    ClientRow, ClientsView, FirstView, GroupWireView, LiveState, LiveView, WireClass, WireDetail,
    WireQuery, WireRefusal, WireRow, WireWindow,
};
use crate::pages::live::RequestLimits;
use crate::recorder::errors;

mod rows;

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
            code_name: r.code_name.clone(),
            code: r.code,
            client: r.client.clone(),
            at_ms: r.at_ms,
            class: r.class,
        })
        .collect();
    out.push(WireRefusal {
        id: 99,
        code_name: "INVALID_PRODUCER_EPOCH".into(),
        code: 47,
        client: "orders".into(),
        at_ms: at("19:31:08.417"),
        class: WireClass::Refused,
    });
    out
}

/// The Live page in the state `hint` names: `paused`, `quiet`, `load`; anything else is the steady tape.
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
        refused_total: 3,
        denied_total: 0,
        clients: 9,
        last_minute: 14,
        busiest: Some(("clickstream".to_owned(), 412)),
        seen: 2000,
        newest_id: 16,
        now_ms: now(),
        listen: "127.0.0.1:9092".to_owned(),
        started_ms: Some(at("22:12:00.000")),
        older: Some(1),
        held: None,
        paused_at_ms: None,
        clients_ids: [
            "admin-cli",
            "catalog",
            "fulfilment",
            "notifications",
            "orders",
            "payments",
            "search-indexer",
        ]
        .map(str::to_owned)
        .to_vec(),
        topics_seen: [
            "clickstream",
            "inventory.levels",
            "orders",
            "payments.settled",
        ]
        .map(str::to_owned)
        .to_vec(),
    };
    match hint {
        Some("paused") => {
            v.rows = steady.iter().take(7).cloned().collect();
            v.matched = 7;
            v.held = Some(17);
            v.paused_at_ms = Some(at("22:14:23.600"));
            v.query.until = Some(7);
        }
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

/// The Clients drawer.
#[must_use]
pub fn clients() -> ClientsView {
    let row = |c: &str,
               tag: &str,
               sw: &str,
               peer: &str,
               more: usize,
               apis: &[(&'static str, i16)],
               age_s: i64| ClientRow {
        client: c.to_owned(),
        tag: Some(tag.to_owned()),
        software: (!sw.is_empty()).then(|| sw.to_owned()),
        peer: Some(peer.to_owned()),
        more,
        apis: apis.to_vec(),
        principal: "User:ANONYMOUS".to_owned(),
        last_ms: now() - age_s * 1000,
        requests: 100,
        errors: 0,
    };
    ClientsView {
        now_ms: now(),
        connections: 12,
        rows: vec![
            row(
                "payments",
                "go",
                "franz-go",
                "127.0.0.1:50388",
                3,
                &[
                    ("Produce", 9),
                    ("OffsetCommit", 9),
                    ("ConsumerGroupHeartbeat", 0),
                ],
                1,
            ),
            row(
                "catalog",
                "go",
                "",
                "127.0.0.1:50391",
                0,
                &[("Produce", 9), ("Metadata", 12)],
                3,
            ),
            row(
                "fulfilment",
                "py",
                "confluent-kafka (librdkafka)",
                "127.0.0.1:50402",
                1,
                &[
                    ("Fetch", 12),
                    ("JoinGroup", 9),
                    ("LeaveGroup", 5),
                    ("Heartbeat", 4),
                ],
                2,
            ),
            row(
                "orders",
                "go",
                "sarama",
                "127.0.0.1:50412",
                1,
                &[("Produce", 9), ("AddPartitionsToTxn", 4), ("EndTxn", 4)],
                3,
            ),
            row(
                "notifications",
                "ts",
                "kafkajs",
                "127.0.0.1:50420",
                0,
                &[("Produce", 9), ("Fetch", 12), ("Heartbeat", 4)],
                40,
            ),
            row(
                "search-indexer",
                "go",
                "",
                "127.0.0.1:50431",
                0,
                &[("Fetch", 12), ("OffsetCommit", 9), ("Heartbeat", 4)],
                7,
            ),
            row(
                "image-worker",
                "go",
                "",
                "127.0.0.1:50444",
                0,
                &[("Metadata", 12)],
                360,
            ),
            row(
                "report-job",
                "py",
                "",
                "127.0.0.1:50452",
                0,
                &[("Fetch", 12)],
                6720,
            ),
            row(
                "admin-cli",
                "kcat",
                "librdkafka",
                "127.0.0.1:50731",
                0,
                &[("Produce", 9), ("Metadata", 12), ("ApiVersions", 3)],
                4,
            ),
        ],
    }
}

/// The first-request page.
#[must_use]
pub fn first() -> FirstView {
    let a = wr(&Spec {
        id: 1,
        time: "21:52:07.118",
        client: "admin-cli",
        tag: "kcat",
        api: "ApiVersions",
        version: 3,
        target: "-",
        sub: None,
        class: WireClass::Done,
        code: 0,
        detail: "Kafka 4 profile",
        took_ms: 1,
    });
    let m = wr(&Spec {
        id: 2,
        time: "21:52:07.121",
        client: "admin-cli",
        tag: "kcat",
        api: "Metadata",
        version: 12,
        target: "all topics",
        sub: None,
        class: WireClass::Done,
        code: 0,
        detail: "9 topics",
        took_ms: 1,
    });
    FirstView {
        client: "admin-cli".into(),
        software: Some("librdkafka".into()),
        peer: Some("127.0.0.1:50731".into()),
        first_ms: at("21:52:07.118"),
        rows: vec![m, a],
        all_answered: true,
        refused: 0,
        topics_listed: Some(9),
        profile: "Kafka 4 profile".into(),
        listen: "127.0.0.1:9092".into(),
        versions: vec![("ApiVersions", 3), ("Metadata", 12)],
    }
}

/// The rows of a topic's dock (the board's, for `orders`) and its window.
#[must_use]
pub fn topic_wire(topic: &str) -> (Vec<WireRow>, WireWindow) {
    let rows: Vec<WireRow> = steady_rows()
        .into_iter()
        .filter(|r| r.topics.iter().any(|t| t == topic))
        .collect();
    (rows.clone(), window(rows.len().max(1), 2000))
}

/// The rows of a group's wire (the board's, for `fulfilment`) and its window.
#[must_use]
pub fn group_wire(q: &WireQuery) -> GroupWireView {
    let group = q.group.as_str();
    let g = |id, t, api, v, target: &str, sub: &str, class, code, detail: &str, ms| {
        let mut r = wr(&Spec {
            id,
            time: t,
            client: "fulfilment",
            tag: "py",
            api,
            version: v,
            target: group,
            sub: Some(sub),
            class,
            code,
            detail,
            took_ms: ms,
        });
        let _ = target;
        r.group = Some(group.to_owned());
        r.topics = vec![];
        r
    };
    let rows = vec![
        g(
            7,
            "22:14:21.900",
            "Heartbeat",
            4,
            "",
            "member fulfilment-py-1, generation 30",
            WireClass::Told,
            27,
            "",
            1,
        ),
        g(
            6,
            "22:14:19.400",
            "LeaveGroup",
            5,
            "",
            "by fulfilment-py-2",
            WireClass::Done,
            0,
            "",
            1,
        ),
        g(
            5,
            "22:14:14.000",
            "OffsetCommit",
            9,
            "",
            "payments.settled 0 to 2",
            WireClass::Done,
            0,
            "",
            5,
        ),
        g(
            4,
            "22:14:13.600",
            "Heartbeat",
            4,
            "",
            "members py-1 and py-2, generation 30",
            WireClass::Done,
            0,
            "",
            1,
        ),
        g(
            3,
            "22:09:04.900",
            "SyncGroup",
            5,
            "",
            "member fulfilment-py-1 (leader) sends the assignment",
            WireClass::Done,
            0,
            "",
            21,
        ),
        g(
            2,
            "22:09:02.600",
            "JoinGroup",
            9,
            "",
            "member fulfilment-py-2, the group moves to generation 30",
            WireClass::Done,
            0,
            "",
            1302,
        ),
        g(
            1,
            "19:44:52.800",
            "OffsetCommit",
            9,
            "",
            "member fulfilment-py-1, generation 28",
            WireClass::Refused,
            22,
            "Specified generation id is not valid.",
            1,
        ),
    ];
    let mut rows = rows;
    if let Some(h) = rows.get_mut(3) {
        h.count = 71;
        h.since_ms = Some(at("22:09:13.600"));
    }
    let all = rows.len();
    let refused = rows
        .iter()
        .filter(|r| r.class == WireClass::Refused)
        .count();
    let kind = |api: &str| match q.kind.as_str() {
        "join" => matches!(api, "JoinGroup" | "SyncGroup" | "LeaveGroup"),
        "commits" => api == "OffsetCommit",
        "heartbeats" => api == "Heartbeat",
        _ => true,
    };
    rows.retain(|r| kind(r.api) && (!q.refused_only || r.class == WireClass::Refused));
    GroupWireView {
        group: group.to_owned(),
        query: q.clone(),
        rows,
        window: window(7, 2000),
        now_ms: now(),
        all,
        refused,
    }
}

/// Home's strip in the world: nine apps on a wire that reaches back to 19:12, the last request a second ago.
#[must_use]
pub fn spoke() -> crate::model::SpokeView {
    crate::model::SpokeView {
        apps: 9,
        requests: 2000,
        oldest_ms: Some(at("19:12:00.000")),
        last_ms: Some(now() - 1000),
        ago_ms: Some(1000),
        last_client: Some("fulfilment".to_owned()),
    }
}
