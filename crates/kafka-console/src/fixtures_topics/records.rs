//! The records of the world: the ten newest of `orders`, the read_committed view, the two records of the boards, and a record for
//! every row of any topic's table.

use super::head;
use crate::fixtures::ADDRESS;
use crate::format::parse_time;
use crate::gather::decode::shown;
use crate::model::{
    HeaderRow, Hold, MessageQuery, MessagesView, Reach, Reading, RecordRow, RecordTxn, RecordView,
    Shown, Standing, TopicHead,
};

fn json(order: &str, customer: &str) -> String {
    format!(
        r#"{{"order_id":"{order}","customer":"{customer}","items":[{{"sku":"FV-TOM-V","qty":2}}],"total":14.80}}"#
    )
}

/// One row of the table of the world: (partition, offset), the time of day, (key, customer), the value's size.
type Case = ((i32, i64), &'static str, (&'static str, &'static str), u64);

fn text_row(
    ((partition, offset), at, (order, customer), size): Case,
    standing: Standing,
) -> RecordRow {
    let value = json(order, customer);
    let mut v = shown(Some(value.as_bytes()), value.len() as u64);
    v.bytes = size;
    RecordRow {
        partition,
        offset,
        timestamp_ms: parse_time(&format!("2026-11-09 {}", at.get(..8).unwrap_or(at))).unwrap_or(0)
            + at_ms(at),
        key: shown(Some(order.as_bytes()), order.len() as u64),
        value: v,
        standing,
    }
}

/// The milliseconds of a `22:14:21.771`.
fn at_ms(at: &str) -> i64 {
    at.rsplit_once('.')
        .and_then(|(_, ms)| ms.parse().ok())
        .unwrap_or(0)
}

const BINARY: [u8; 52] = [
    0x0a, 0x09, 0x6f, 0x72, 0x64, 0x2d, 0x38, 0x38, 0x34, 0x30, 0x39, 0x12, 0x05, 0x63, 0x2d, 0x34,
    0x34, 0x31, 0x1a, 0x0e, 0x0a, 0x08, 0x46, 0x56, 0x2d, 0x54, 0x4f, 0x4d, 0x2d, 0x56, 0x10, 0x02,
    0x25, 0xcd, 0xcc, 0x6c, 0x40, 0xff, 0x00, 0x9f, 0x3a, 0x7e, 0x01, 0x80, 0xd4, 0x11, 0x5b, 0xe2,
    0x0a, 0x1c, 0x00, 0x7f,
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

/// The ten newest records of `orders`, as the board lists them (the binary one is the ninth).
fn newest() -> Vec<RecordRow> {
    use Standing::{AboveLso, Committed};
    let held: [Case; 4] = [
        ((0, 16_103), "22:14:21.771", ("ord-88418", "c-3092"), 91),
        ((0, 16_102), "22:14:21.770", ("ord-88417", "c-5518"), 88),
        ((0, 16_101), "22:14:21.769", ("ord-88416", "c-7710"), 86),
        ((0, 16_100), "22:14:21.769", ("ord-88415", "c-2204"), 97),
    ];
    let mut rows: Vec<RecordRow> = held.into_iter().map(|c| text_row(c, AboveLso)).collect();
    rows.push(text_row(
        ((2, 16_057), "22:14:19.208", ("ord-88414", "c-1187"), 84),
        Committed,
    ));
    rows.push(text_row(
        ((1, 16_050), "22:14:17.402", ("ord-88412", "c-7710"), 86),
        AboveLso,
    ));
    rows.push(text_row(
        ((0, 16_099), "22:14:11.690", ("ord-88411", "c-4630"), 93),
        AboveLso,
    ));
    rows.push(text_row(
        ((0, 16_098), "22:13:33.208", ("ord-88410", "c-9061"), 90),
        AboveLso,
    ));
    rows.push(binary_row());
    rows.push(text_row(
        ((1, 16_049), "22:12:48.915", ("ord-88407", "c-6602"), 89),
        Committed,
    ));
    rows
}

fn holds(h: &TopicHead) -> Vec<Hold> {
    h.parts
        .iter()
        .map(|p| Hold {
            partition: p.partition,
            high_watermark: p.end,
            last_stable: p.last_stable,
            txn: h
                .txns
                .first()
                .and_then(|t| t.partitions.iter().find(|(tp, _)| *tp == p.partition))
                .and_then(|(_, o)| o.map(|o| ("orders-eos".to_owned(), o))),
        })
        .collect()
}

fn view(
    head: TopicHead,
    query: MessageQuery,
    notes: Vec<String>,
    rows: Vec<RecordRow>,
) -> MessagesView {
    let reach = Reach {
        returned: rows.len(),
        first_offset: rows.iter().map(|r| r.offset).min(),
        last_offset: rows.iter().map(|r| r.offset).max(),
        reached_end: true,
        above_lso: rows
            .iter()
            .filter(|r| r.standing == Standing::AboveLso)
            .count(),
        scanned: None,
        more: head.records() > i64::try_from(rows.len()).unwrap_or(0) && !query.filtered(),
    };
    MessagesView {
        holds: holds(&head),
        head,
        query,
        notes,
        rows,
        reach,
    }
}

fn committed_rows() -> Vec<RecordRow> {
    let mut rows = vec![
        text_row(
            ((2, 16_057), "22:14:19.208", ("ord-88414", "c-1187"), 84),
            Standing::Committed,
        ),
        binary_row(),
        text_row(
            ((1, 16_049), "22:12:48.915", ("ord-88407", "c-6602"), 89),
            Standing::Committed,
        ),
        text_row(
            ((0, 16_097), "22:12:30.118", ("ord-88406", "c-3351"), 92),
            Standing::Committed,
        ),
    ];
    rows.sort_by_key(|r| std::cmp::Reverse(r.timestamp_ms));
    rows
}

/// A topic's Messages for `query`: the board that the query calls for.
#[must_use]
pub fn messages(name: &str, query: MessageQuery, notes: Vec<String>) -> Option<MessagesView> {
    let head = head(name)?;
    if name != "orders" {
        let rows = if head.is_empty() {
            Vec::new()
        } else {
            newest().into_iter().take(query.limit.min(10)).collect()
        };
        return Some(view(head, query, notes, rows));
    }
    let rows = match (query.reading, query.filtered()) {
        (Reading::Committed, _) => committed_rows(),
        (_, true) => newest()
            .into_iter()
            .filter(|r| {
                (query.partitions.is_empty() || query.partitions.contains(&r.partition))
                    && r.partition == 0
                    && r.offset >= query.offset.unwrap_or(0)
            })
            .collect(),
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
            HeaderRow {
                key: "content-type".to_owned(),
                value: field("application/json"),
            },
            HeaderRow {
                key: "traceparent".to_owned(),
                value: field("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
            },
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

/// The record `FV-BAN-X` deleted: a key and no value.
fn tombstone_record() -> RecordView {
    let mut v = orders_record();
    "inventory.levels".clone_into(&mut v.topic);
    (v.partition, v.offset) = (0, 36_402);
    v.key = Shown {
        kind: crate::model::Kind::Text,
        bytes: 8,
        text: "FV-BAN-X".to_owned(),
        clipped: false,
        preview: "FV-BAN-X".to_owned(),
    };
    v.value = Shown {
        kind: crate::model::Kind::Null,
        bytes: 0,
        text: String::new(),
        clipped: false,
        preview: String::new(),
    };
    v.headers.clear();
    v.producer = None;
    v.transactional = false;
    v.txn = None;
    v
}

/// One record of the world. A row of any topic's table has a page.
#[must_use]
pub fn record(name: &str, partition: i32, offset: i64) -> Option<RecordView> {
    match (name, partition, offset) {
        ("orders", 1, 16_050) => return Some(orders_record()),
        ("orders", 2, 16_056) => return Some(binary_record()),
        // The deletion the Log page of `inventory.levels` links to (slice 9g).
        ("inventory.levels", 0, 36_402) => return Some(tombstone_record()),
        _ => {}
    }
    let mut all = messages(name, MessageQuery::default(), Vec::new())?.rows;
    all.extend(committed_rows());
    let found = all
        .into_iter()
        .find(|r| r.partition == partition && r.offset == offset)?;
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
