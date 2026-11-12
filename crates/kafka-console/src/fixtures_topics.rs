//! The "larder" world of the topic boards as view-models: `orders` with its open transaction, `orders.dlq` that is empty,
//! `notifications.email` with an offline partition, the list of nine topics, and the two records. Used by the tests and by the
//! `boards` example. The headline numbers are checked against the boards' text by `tests/boards_topics.rs`.

use crate::format::parse_time;
use crate::model::{Cleanup, OpenTxn, PartitionRow, TabCounts, TopicHead, TopicRow, TopicsView};

/// The moment of the world: Nov 9, 22:14:25 UTC.
#[must_use]
pub fn now() -> i64 {
    parse_time("2026-11-09 22:14:25").unwrap_or(0)
}

const MIB: u64 = 1 << 20;

/// `n` tenths of a MiB, rounded up so that the size reads as the board says it.
fn tenths(n: u64) -> u64 {
    (n * MIB).div_ceil(10)
}

#[allow(clippy::too_many_arguments)]
fn topic(
    name: &str,
    partitions: i32,
    cleanup: Cleanup,
    (bytes, ago_ms): (u64, Option<i64>),
    read_by: &[&str],
) -> TopicRow {
    TopicRow {
        name: name.to_owned(),
        internal: false,
        partitions,
        cleanup,
        txn_open: false,
        empty: bytes == 0,
        retention_ms: None,
        segments: Some(1),
        read_by: read_by.iter().map(|s| (*s).to_owned()).collect(),
        bytes: Some(bytes),
        last_write_ms: ago_ms.map(|a| now() - a),
        offline: 0,
    }
}

fn rows() -> Vec<TopicRow> {
    let mut orders = topic(
        "orders",
        3,
        Cleanup::Delete,
        (tenths(79), Some(4_000)),
        &["payments-workers"],
    );
    orders.txn_open = true;
    let mut clicks = topic(
        "clickstream",
        6,
        Cleanup::Delete,
        (tenths(1286), Some(200)),
        &["search-indexer"],
    );
    clicks.retention_ms = Some(3_600_000);
    let mut customers = topic(
        "customer.profiles",
        1,
        Cleanup::Compact,
        (tenths(88), Some(6 * 3_600_000)),
        &[],
    );
    customers.segments = Some(3);
    vec![
        orders,
        topic(
            "payments.settled",
            4,
            Cleanup::Delete,
            (tenths(24), Some(11_000)),
            &["fulfilment"],
        ),
        topic(
            "inventory.levels",
            1,
            Cleanup::Compact,
            (tenths(397), Some(3_000)),
            &["search-indexer"],
        ),
        customers,
        topic(
            "pick-lists",
            1,
            Cleanup::Delete,
            (tenths(5), Some(41_000)),
            &["fulfilment"],
        ),
        topic(
            "pick-lists.dlq",
            1,
            Cleanup::Delete,
            (1434, Some(3 * 86_400_000)),
            &[],
        ),
        topic(
            "notifications.email",
            3,
            Cleanup::Delete,
            (tenths(21), Some(60_000)),
            &["notifications"],
        ),
        clicks,
        topic("orders.dlq", 1, Cleanup::Delete, (0, None), &[]),
    ]
}

/// The list, for `filter` and `internal`; `hint` `none` is the broker with no topics.
#[must_use]
pub fn topics(filter: &str, internal: bool, hint: Option<&str>) -> TopicsView {
    let all = if hint == Some("none") {
        Vec::new()
    } else {
        rows()
    };
    let total = all.len();
    let partitions = all
        .iter()
        .map(|r| usize::try_from(r.partitions).unwrap_or(0))
        .sum();
    let mut shown: Vec<TopicRow> = all
        .into_iter()
        .filter(|r| filter.is_empty() || r.name.contains(filter))
        .collect();
    let newest = filter
        .is_empty()
        .then(|| {
            shown
                .iter()
                .filter_map(|r| r.last_write_ms.map(|m| (r.name.clone(), m)))
                .max_by_key(|(_, m)| *m)
        })
        .flatten();
    if internal {
        shown.push(TopicRow::internal());
    }
    TopicsView {
        rows: shown,
        total,
        partitions,
        filter: filter.to_owned(),
        show_internal: internal,
        groups: if total == 0 { 0 } else { 4 },
        newest,
        now_ms: now(),
        truncated: false,
    }
}

fn part(partition: i32, end: i64, last_stable: i64, mib_tenths: u64) -> PartitionRow {
    PartitionRow {
        partition,
        start: 0,
        end,
        last_stable,
        bytes: Some((mib_tenths * MIB).div_ceil(10)),
        last_write_ms: Some(now() - 4_000),
        offline: None,
    }
}

fn orders() -> TopicHead {
    TopicHead {
        name: "orders".to_owned(),
        cleanup: Cleanup::Delete,
        retention_ms: Some(604_800_000),
        parts: vec![
            part(0, 16_104, 16_098, 27),
            part(1, 16_051, 16_050, 26),
            part(2, 16_058, 16_058, 26),
        ],
        txns: vec![OpenTxn {
            id: "orders-eos".to_owned(),
            producer_id: 4017,
            producer_epoch: 2,
            since_ms: parse_time("2026-11-09 22:13:33").unwrap_or(0),
            timeout_ms: 60_000,
            partitions: vec![(0, Some(16_098)), (1, Some(16_050))],
        }],
        tabs: TabCounts {
            consumers: 2,
            producers: 3,
        },
        now_ms: now(),
    }
}

fn dlq() -> TopicHead {
    TopicHead {
        name: "orders.dlq".to_owned(),
        cleanup: Cleanup::Delete,
        retention_ms: Some(604_800_000),
        parts: vec![PartitionRow {
            partition: 0,
            start: 0,
            end: 0,
            last_stable: 0,
            bytes: Some(0),
            last_write_ms: None,
            offline: None,
        }],
        txns: Vec::new(),
        tabs: TabCounts::default(),
        now_ms: now(),
    }
}

fn notifications() -> TopicHead {
    let mut parts = vec![
        part(0, 4_120, 4_120, 7),
        part(1, 2_044, 2_044, 7),
        part(2, 4_096, 4_096, 7),
    ];
    if let Some(p) = parts.get_mut(1) {
        p.offline = Some("Input/output error (os error 5)".to_owned());
    }
    TopicHead {
        name: "notifications.email".to_owned(),
        cleanup: Cleanup::Delete,
        retention_ms: Some(604_800_000),
        parts,
        txns: Vec::new(),
        tabs: TabCounts {
            consumers: 1,
            producers: 1,
        },
        now_ms: now(),
    }
}

/// A topic of the list that has no board of its own: one partition's worth of plain records.
fn generic(r: &TopicRow) -> TopicHead {
    let end = if r.empty { 0 } else { 1_200 };
    TopicHead {
        name: r.name.clone(),
        cleanup: r.cleanup,
        retention_ms: Some(r.retention_ms.unwrap_or(604_800_000)),
        parts: (0..r.partitions)
            .map(|p| PartitionRow {
                partition: p,
                start: 0,
                end,
                last_stable: end,
                bytes: r.bytes,
                last_write_ms: r.last_write_ms,
                offline: None,
            })
            .collect(),
        txns: Vec::new(),
        tabs: TabCounts {
            consumers: r.read_by.len(),
            producers: 0,
        },
        now_ms: now(),
    }
}

/// The head of a topic of the world.
#[must_use]
pub fn head(name: &str) -> Option<TopicHead> {
    match name {
        "orders" => Some(orders()),
        "orders.dlq" => Some(dlq()),
        "notifications.email" => Some(notifications()),
        other => rows().iter().find(|r| r.name == other).map(generic),
    }
}

mod records;

pub use records::{listen, messages, record};
