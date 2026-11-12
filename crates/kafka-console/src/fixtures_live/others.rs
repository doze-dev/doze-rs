//! The clients, the first request, the docks and Home's strip of the world.
#![allow(clippy::too_many_lines, clippy::too_many_arguments)]

use super::{Spec, at, now, steady_rows, window, wr};
use crate::model::{
    ClientRow, ClientsView, Facet, Facets, FirstView, GroupWireView, WireClass, WireQuery, WireRow,
    WireWindow,
};

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

/// The filters' options: the fixture tape's apps, APIs and topics (and the few more the larder's world has), each with its rows.
pub fn facets(rows: &[WireRow]) -> Facets {
    let list = |names: &[&str], of: &dyn Fn(&WireRow, &str) -> bool| {
        names
            .iter()
            .map(|n| Facet {
                value: (*n).to_owned(),
                count: rows.iter().filter(|r| of(r, n)).count(),
            })
            .collect()
    };
    Facets {
        clients: list(
            &[
                "admin-cli",
                "catalog",
                "fulfilment",
                "notifications",
                "orders",
                "payments",
                "search-indexer",
            ],
            &|r, n| r.client == n,
        ),
        apis: list(
            &[
                "Fetch",
                "Metadata",
                "OffsetCommit",
                "Produce",
                "InitProducerId",
                "ApiVersions",
            ],
            &|r, n| r.api == n,
        ),
        topics: list(
            &[
                "clickstream",
                "inventory.levels",
                "orders",
                "payments.settled",
            ],
            &|r, n| r.topics.iter().any(|t| t == n),
        ),
    }
}
