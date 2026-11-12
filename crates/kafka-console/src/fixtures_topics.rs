//! The "larder" world of the topic boards as view-models: `orders` with its open transaction, `orders.dlq` that is empty,
//! `notifications.email` with an offline partition, the list of nine topics, and the two records. Used by the tests and by the
//! `boards` example. The headline numbers are checked against the boards' text by `tests/boards_topics.rs`.

use crate::fixtures::ADDRESS;
use crate::format::parse_time;
use crate::gather::decode::shown;
use crate::model::{
    Cleanup, HeaderRow, Hold, MessageQuery, MessagesView, OpenTxn, PartitionRow, Reach, Reading,
    RecordRow, RecordTxn, RecordView, Shown, Standing, TabCounts, TopicHead, TopicRow, TopicsView,
};

/// The moment of the world: Nov 9, 22:14:25 UTC.
#[must_use]
pub fn now() -> i64 {
    parse_time("2026-11-09 22:14:25").unwrap_or(0)
}

const MIB: u64 = 1 << 20;

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
    let mut orders = topic("orders", 3, Cleanup::Delete, (79 * MIB / 10, Some(4_000)), &["payments-workers"]);
    orders.txn_open = true;
    let mut clicks = topic("clickstream", 6, Cleanup::Delete, (1286 * MIB / 10, Some(200)), &["search-indexer"]);
    clicks.retention_ms = Some(3_600_000);
    let mut customers = topic("customer.profiles", 1, Cleanup::Compact, (88 * MIB / 10, Some(6 * 3_600_000)), &[]);
    customers.segments = Some(3);
    vec![
        orders,
        topic("payments.settled", 4, Cleanup::Delete, (24 * MIB / 10, Some(11_000)), &["fulfilment"]),
        topic("inventory.levels", 1, Cleanup::Compact, (397 * MIB / 10, Some(3_000)), &["search-indexer"]),
        customers,
        topic("pick-lists", 1, Cleanup::Delete, (MIB / 2, Some(41_000)), &["fulfilment"]),
        topic("pick-lists.dlq", 1, Cleanup::Delete, (1434, Some(3 * 86_400_000)), &[]),
        topic("notifications.email", 3, Cleanup::Delete, (21 * MIB / 10, Some(60_000)), &["notifications"]),
        clicks,
        topic("orders.dlq", 1, Cleanup::Delete, (0, None), &[]),
    ]
}

/// The list, for `filter` and `internal`; `hint` `none` is the broker with no topics.
#[must_use]
pub fn topics(filter: &str, internal: bool, hint: Option<&str>) -> TopicsView {
    let all = if hint == Some("none") { Vec::new() } else { rows() };
    let total = all.len();
    let partitions = all.iter().map(|r| usize::try_from(r.partitions).unwrap_or(0)).sum();
    let mut shown: Vec<TopicRow> = all.into_iter().filter(|r| filter.is_empty() || r.name.contains(filter)).collect();
    let newest = filter.is_empty().then(|| shown.iter().filter_map(|r| r.last_write_ms.map(|m| (r.name.clone(), m))).max_by_key(|(_, m)| *m)).flatten();
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
        parts: vec![part(0, 16_104, 16_098, 27), part(1, 16_051, 16_050, 26), part(2, 16_058, 16_058, 26)],
        txns: vec![OpenTxn {
            id: "orders-eos".to_owned(),
            producer_id: 4017,
            producer_epoch: 2,
            since_ms: parse_time("2026-11-09 22:13:33").unwrap_or(0),
            timeout_ms: 60_000,
            partitions: vec![(0, Some(16_098)), (1, Some(16_050))],
        }],
        tabs: TabCounts { consumers: 2, producers: 3 },
        now_ms: now(),
    }
}

fn dlq() -> TopicHead {
    TopicHead {
        name: "orders.dlq".to_owned(),
        cleanup: Cleanup::Delete,
        retention_ms: Some(604_800_000),
        parts: vec![PartitionRow { partition: 0, start: 0, end: 0, last_stable: 0, bytes: Some(0), last_write_ms: None, offline: None }],
        txns: Vec::new(),
        tabs: TabCounts::default(),
        now_ms: now(),
    }
}

fn notifications() -> TopicHead {
    let mut parts = vec![part(0, 4_120, 4_120, 7), part(1, 2_044, 2_044, 7), part(2, 4_096, 4_096, 7)];
    parts[1].offline = Some("Input/output error (os error 5)".to_owned());
    TopicHead {
        name: "notifications.email".to_owned(),
        cleanup: Cleanup::Delete,
        retention_ms: Some(604_800_000),
        parts,
        txns: Vec::new(),
        tabs: TabCounts { consumers: 1, producers: 1 },
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
        tabs: TabCounts { consumers: r.read_by.len(), producers: 0 },
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

fn json(order: &str, customer: &str) -> String {
    format!(r#"{{"order_id":"{order}","customer":"{customer}","items":[{{"sku":"FV-TOM-V","qty":2}}],"total":14.80}}"#)
}

fn text_row(partition: i32, offset: i64, at: &str, order: &str, customer: &str, size: u64, standing: Standing) -> RecordRow {
    let value = json(order, customer);
    let mut v = shown(Some(value.as_bytes()), value.len() as u64);
    v.bytes = size;
    RecordRow {
        partition,
        offset,
        timestamp_ms: parse_time(&format!("2026-11-09 {at}")).unwrap_or(0) + at_ms(at),
        key: shown(Some(order.as_bytes()), order.len() as u64),
        value: v,
        standing,
    }
}

/// The milliseconds of a `22:14:21.771`.
fn at_ms(at: &str) -> i64 {
    at.rsplit_once('.').and_then(|(_, ms)| ms.parse().ok()).unwrap_or(0)
}

const BINARY: [u8; 52] = [
    0x0a, 0x09, 0x6f, 0x72, 0x64, 0x2d, 0x38, 0x38, 0x34, 0x30, 0x39, 0x12, 0x05,
    0x63, 0x2d, 0x34, 0x34, 0x31, 0x1a, 0x0e, 0x0a, 0x08, 0x46, 0x56, 0x2d, 0x54,
    0x4f, 0x4d, 0x2d, 0x56, 0x10, 0x02, 0x25, 0xcd, 0xcc, 0x6c, 0x40, 0xff, 0x00,
    0x9f, 0x3a, 0x7e, 0x01, 0x80, 0xd4, 0x11, 0x5b, 0xe2, 0x0a, 0x1c, 0x00, 0x7f,
];

fn binary_row() -> RecordRow {
    RecordRow {
        partition: 2,
        offset: 16_056,
        timestamp_ms: parse_time("2026-11-09 22:13:02").unwrap_or(0) + 447,
        key: shown(Some(b"ord-88409"), 9),
        value: shown(Some(&BINARY), 52),
        standing: Standing::Committed,
    }
}

fn newest() -> Vec<RecordRow> {
    use Standing::{AboveLso, Committed};
    vec![
        text_row(0, 16_103, "22:14:21.771", "ord-88418", "c-3092", 91, AboveLso),
        text_row(0, 16_102, "22:14:21.770", "ord-88417", "c-5518", 88, AboveLso),
        text_row(0, 16_101, "22:14:21.769", "ord-88416", "c-7710", 86, AboveLso),
        text_row(0, 16_100, "22:14:21.769", "ord-88415", "c-2204", 97, AboveLso),
        text_row(2, 16_057, "22:14:19.208", "ord-88414", "c-1187", 84, Committed),
        text_row(1, 16_050, "22:14:17.402", "ord-88412", "c-7710", 86, AboveLso),
        text_row(0, 16_099, "22:14:11.690", "ord-88411", "c-4630", 93, AboveLso),
        text_row(0, 16_098, "22:13:33.208", "ord-88410", "c-9061", 90, AboveLso),
        binary_row(),
        text_row(1, 16_049, "22:12:48.915", "ord-88407", "c-6602", 89, Committed),
    ]
}

fn holds(h: &TopicHead) -> Vec<Hold> {
    h.parts
        .iter()
        .map(|p| Hold {
            partition: p.partition,
            high_watermark: p.end,
            last_stable: p.last_stable,
            txn: h.txns.first().and_then(|t| t.partitions.iter().find(|(tp, _)| *tp == p.partition)).and_then(|(_, o)| o.map(|o| ("orders-eos".to_owned(), o))),
        })
        .collect()
}

fn view(head: TopicHead, query: MessageQuery, notes: Vec<String>, rows: Vec<RecordRow>) -> MessagesView {
    let reach = Reach {
        returned: rows.len(),
        first_offset: rows.iter().map(|r| r.offset).min(),
        last_offset: rows.iter().map(|r| r.offset).max(),
        reached_end: true,
        above_lso: rows.iter().filter(|r| r.standing == Standing::AboveLso).count(),
        scanned: None,
        more: false,
    };
    MessagesView { holds: holds(&head), head, query, notes, rows, reach }
}

fn committed_rows() -> Vec<RecordRow> {
    let mut rows = vec![
        text_row(2, 16_057, "22:14:19.208", "ord-88414", "c-1187", 84, Standing::Committed),
        binary_row(),
        text_row(1, 16_049, "22:12:48.915", "ord-88407", "c-6602", 89, Standing::Committed),
        text_row(0, 16_097, "22:12:30.118", "ord-88406", "c-3351", 92, Standing::Committed),
    ];
    rows.sort_by_key(|r| std::cmp::Reverse(r.timestamp_ms));
    rows
}

/// A topic's Messages for `query`: the board that the query calls for.
#[must_use]
pub fn messages(name: &str, query: MessageQuery, notes: Vec<String>) -> Option<MessagesView> {
    let head = head(name)?;
    if name != "orders" {
        let rows = if head.is_empty() { Vec::new() } else { newest().into_iter().take(query.limit.min(10)).collect() };
        return Some(view(head, query, notes, rows));
    }
    let rows = match (query.reading, query.filtered()) {
        (Reading::Committed, _) => committed_rows(),
        (_, true) => newest().into_iter().filter(|r| r.partition == query.partition.unwrap_or(0) && r.partition == 0 && r.offset >= query.offset.unwrap_or(0)).collect(),
        _ => newest(),
    };
    Some(view(head, query, notes, rows))
}

fn field(s: &str) -> Shown {
    shown(Some(s.as_bytes()), s.len() as u64)
}

fn orders_record() -> RecordView {
    let value = json("ord-88412", "c-7710");
    let mut v = shown(Some(value.as_bytes()), value.len() as u64);
    v.bytes = 86;
    RecordView {
        topic: "orders".to_owned(),
        partition: 1,
        offset: 16_050,
        last_stable: 16_050,
        key: field("ord-88412"),
        value: v,
        headers: vec![
            HeaderRow { key: "content-type".to_owned(), value: field("application/json") },
            HeaderRow { key: "traceparent".to_owned(), value: field("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01") },
        ],
        timestamp_ms: parse_time("2026-11-09 22:14:17").unwrap_or(0) + 402,
        log_append_time: false,
        codec: "none".to_owned(),
        producer: Some((4017, 2, 22_918)),
        transactional: true,
        txn: Some(RecordTxn {
            id: Some("orders-eos".to_owned()),
            state: Some("Ongoing".to_owned()),
            since_ms: parse_time("2026-11-09 22:13:33"),
            timeout_ms: Some(60_000),
        }),
        standing: Standing::AboveLso,
    }
}

fn binary_record() -> RecordView {
    let row = binary_row();
    RecordView {
        topic: "orders".to_owned(),
        partition: 2,
        offset: 16_056,
        last_stable: 16_058,
        key: row.key,
        value: row.value,
        headers: Vec::new(),
        timestamp_ms: row.timestamp_ms,
        log_append_time: false,
        codec: "none".to_owned(),
        producer: None,
        transactional: false,
        txn: None,
        standing: Standing::Committed,
    }
}

/// One record of the world. A row of any topic's table has a page.
#[must_use]
pub fn record(name: &str, partition: i32, offset: i64) -> Option<RecordView> {
    match (name, partition, offset) {
        ("orders", 1, 16_050) => return Some(orders_record()),
        ("orders", 2, 16_056) => return Some(binary_record()),
        _ => {}
    }
    let found = messages(name, MessageQuery::default(), Vec::new())?.rows.into_iter().find(|r| r.partition == partition && r.offset == offset)?;
    let mut v = orders_record();
    v.topic = name.to_owned();
    v.partition = partition;
    v.offset = offset;
    v.key = found.key;
    v.value = found.value;
    v.timestamp_ms = found.timestamp_ms;
    v.standing = found.standing;
    v.headers.clear();
    v.producer = None;
    v.transactional = false;
    v.txn = None;
    Some(v)
}

/// The address the world's commands name.
#[must_use]
pub fn listen() -> &'static str {
    ADDRESS
}
