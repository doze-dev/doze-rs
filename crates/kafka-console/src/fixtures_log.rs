//! The "larder" world of the Log and Producers boards as view-models: `inventory.levels` (compacted, due), `customer.profiles`
//! (compacted, not due), `orders` (deletes by age, with an open transaction) and `orders.dlq` (empty). Used by the tests and by
//! the `boards` example. The headline numbers are checked against the boards' text by `tests/boards_log.rs`.

use crate::format::parse_time;
use crate::model::{
    Cleanup, LogCleaner, LogCompaction, LogDurability, LogLimit, LogOther, LogOtherLink, LogPass,
    LogPlanner, LogRetentionView, LogRound, LogRules, LogSegment, LogStop, LogTombstone, LogView,
    ProdAborted, ProdBatch, ProdCell, ProdRow, ProdTail, ProdTxn, ProducersView, TxnPart, TxnRow,
    TxnView,
};

const MIB: u64 = 1 << 20;
const SEGMENT_BYTES: u64 = 8 * MIB;

/// The moment of the world: Nov 9, 22:14:14 UTC (41 s into the open transaction's 60 s).
#[must_use]
pub fn now() -> i64 {
    at("2026-11-09 22:14:14")
}

fn at(text: &str) -> i64 {
    parse_time(text).unwrap_or(0)
}

fn tenths(n: u64) -> u64 {
    (n * MIB).div_ceil(10)
}

/// `kept` per cent of the offsets stay: the rest are removed in seven even chunks, as the board hatches them.
fn gaps(base: i64, span: i64, kept: i64) -> Vec<(i64, i64)> {
    let removed = span * (100 - kept) / 100;
    let each = removed / 7;
    let step = span / 7;
    (0..7).map(|i| (base + i * step + step / 8, each)).collect()
}

fn seg(base: i64, next: i64, bytes: u64, newest: &str, kept: Option<i64>) -> LogSegment {
    let span = next - base;
    let g = kept.map_or_else(Vec::new, |k| gaps(base, span, k));
    let removed: i64 = g.iter().map(|x| x.1).sum();
    LogSegment {
        base,
        next,
        bytes,
        newest_ms: Some(at(newest)),
        records: u64::try_from(span - removed).unwrap_or(0),
        gaps: g,
        unplaced: 0,
        exact: true,
        active: false,
    }
}

fn active(mut s: LogSegment) -> LogSegment {
    s.active = true;
    s
}

fn durability() -> LogDurability {
    LogDurability {
        offline: None,
        stop: LogStop::Clean,
        fsync_ms: 500,
    }
}

fn base_view(topic: &str, partitions: i32, cleanup: Cleanup) -> LogView {
    LogView {
        topic: topic.to_owned(),
        partition: 0,
        partitions,
        now_ms: now(),
        cleanup,
        start: 0,
        end: 0,
        last_stable: 0,
        never_written: false,
        segments: Vec::new(),
        partial: false,
        segment_bytes: SEGMENT_BYTES,
        index_interval_bytes: 4096,
        segment_ms: Some(7 * 86_400_000),
        checkpoint: 0,
        tombstones: Vec::new(),
        compaction: None,
        retention: None,
        durability: durability(),
        others: Vec::new(),
    }
}

fn rules() -> LogRules {
    LogRules {
        min_dirty_ratio: 0.5,
        min_lag_ms: 0,
        max_lag_ms: None,
        delete_retention_ms: 86_400_000,
    }
}

fn others() -> Vec<LogOther> {
    let o = |topic: &str, cleanup, line: &str, sub: &str, link| LogOther {
        topic: topic.to_owned(),
        cleanup,
        line: line.to_owned(),
        sub: Some(sub.to_owned()),
        link,
    };
    vec![
        o(
            "customer.profiles",
            Cleanup::Compact,
            "3 segments, 31 % dirty: not due",
            "needs more than 50 %",
            LogOtherLink::NotDue,
        ),
        o(
            "orders",
            Cleanup::Delete,
            "retention 7 d: segment 1 of partition 0 goes at 22:22",
            "checked every 5 m",
            LogOtherLink::Retention,
        ),
        o(
            "orders.dlq",
            Cleanup::Delete,
            "empty: no records yet",
            "one active segment, nothing else",
            LogOtherLink::Empty,
        ),
    ]
}

fn inventory() -> LogView {
    let mut v = base_view("inventory.levels", 1, Cleanup::Compact);
    v.segments = vec![
        seg(0, 9812, tenths(32), "2026-11-06 20:41", Some(41)),
        seg(9812, 19_603, tenths(36), "2026-11-07 03:12", Some(44)),
        seg(19_603, 29_420, tenths(39), "2026-11-07 11:55", Some(47)),
        seg(29_420, 39_195, tenths(44), "2026-11-08 02:30", Some(52)),
        seg(39_195, 48_877, SEGMENT_BYTES, "2026-11-08 15:08", None),
        seg(48_877, 55_210, SEGMENT_BYTES, "2026-11-09 07:44", None),
        seg(55_210, 61_300, SEGMENT_BYTES, "2026-11-09 21:50", None),
        active(seg(61_300, 61_904, tenths(6), "2026-11-09 22:14:10", None)),
    ];
    (v.end, v.last_stable, v.checkpoint) = (61_904, 61_904, 39_195);
    v.tombstones = vec![LogTombstone {
        key: "FV-BAN-X".to_owned(),
        binary: false,
        offset: 36_402,
        written_ms: at("2026-11-09 09:12"),
        goes_after_ms: at("2026-11-10 09:12"),
    }];
    v.compaction = Some(LogCompaction {
        rules: rules(),
        every_ms: 15_000,
        looked_at_ms: Some(now()),
        planner: LogPlanner {
            upto: 61_300,
            limit: LogLimit::Active,
            first_dirty: 39_195,
            dirty_start: 39_195,
            dirty_bytes: 3 * SEGMENT_BYTES,
            clean_bytes: tenths(32) + tenths(36) + tenths(39) + tenths(44),
            waited_too_long: false,
            due: true,
        },
        cleaner: LogCleaner::Watching,
        last: Some(LogRound {
            at_ms: at("2026-11-09 21:31:14"),
            took_ms: 1400,
            read: 9775,
            kept: 5120,
            replaced: 1,
            clean_to: 39_195,
        }),
    });
    v.others = others();
    v
}

fn customer() -> LogView {
    let mut v = base_view("customer.profiles", 1, Cleanup::Compact);
    v.segments = vec![
        seg(0, 1500, tenths(61), "2026-11-08 10:00", Some(41)),
        seg(1500, 3400, tenths(27), "2026-11-09 18:00", None),
        active(seg(3400, 4402, tenths(10), "2026-11-09 22:14:00", None)),
    ];
    (v.end, v.last_stable, v.checkpoint) = (4402, 4402, 1500);
    v.compaction = Some(LogCompaction {
        rules: rules(),
        every_ms: 15_000,
        looked_at_ms: Some(now()),
        planner: LogPlanner {
            upto: 3400,
            limit: LogLimit::Active,
            first_dirty: 1500,
            dirty_start: 1500,
            dirty_bytes: tenths(27),
            clean_bytes: tenths(61),
            waited_too_long: false,
            due: false,
        },
        cleaner: LogCleaner::Watching,
        last: None,
    });
    v.others = others();
    v
}

fn orders() -> LogView {
    let mut v = base_view("orders", 3, Cleanup::Delete);
    v.segments = vec![
        seg(0, 5980, tenths(9), "2026-11-02 22:19", None),
        active(seg(5980, 16_104, tenths(17), "2026-11-09 22:14:10", None)),
    ];
    (v.end, v.last_stable) = (16_104, 16_098);
    v.segment_bytes = 128 * MIB;
    v.retention = Some(LogRetentionView {
        max_age_ms: Some(7 * 86_400_000),
        max_bytes: None,
        every_ms: 300_000,
        would_delete: 0,
        last_pass: Some(LogPass {
            at_ms: at("2026-11-09 22:12:00"),
            looked: 3,
            moved: Vec::new(),
            more: 0,
        }),
        moved_total: 0,
        moved_at_ms: None,
    });
    v
}

fn dlq() -> LogView {
    let mut v = base_view("orders.dlq", 1, Cleanup::Delete);
    v.never_written = true;
    v.segment_bytes = 128 * MIB;
    v
}

fn generic(name: &str) -> LogView {
    let mut v = orders();
    name.clone_into(&mut v.topic);
    v.partitions = 1;
    v.last_stable = v.end;
    v
}

/// The Log page of a topic of the world; `None` for a name that is not one.
#[must_use]
pub fn log(topic: &str, partition: i32) -> Option<LogView> {
    let mut v = match topic {
        "inventory.levels" => inventory(),
        "customer.profiles" => customer(),
        "orders" => orders(),
        "orders.dlq" => dlq(),
        "payments.settled"
        | "pick-lists"
        | "pick-lists.dlq"
        | "notifications.email"
        | "clickstream" => generic(topic),
        _ => return None,
    };
    v.partition = partition.clamp(0, v.partitions - 1);
    Some(v)
}

fn batch(seq: (i32, i32), offsets: (i64, i64), at_text: &str, open: bool) -> ProdBatch {
    ProdBatch {
        first_sequence: seq.0,
        last_sequence: seq.1,
        first_offset: offsets.0,
        last_offset: offsets.1,
        at_ms: at(at_text),
        open,
    }
}

fn txn() -> TxnRow {
    TxnRow {
        id: "orders-eos".to_owned(),
        state: "Ongoing".to_owned(),
        producer_id: 4017,
        producer_epoch: 2,
        since_ms: at("2026-11-09 22:13:33"),
        timeout_ms: 60_000,
        partitions: vec![("orders".to_owned(), 0), ("orders".to_owned(), 1)],
    }
}

fn tail() -> ProdTail {
    let cells = (16_085..16_104)
        .map(|o| match o {
            16_089 | 16_092 | 16_097 => ProdCell::Marker,
            o if o >= 16_098 => ProdCell::Held,
            _ => ProdCell::Committed,
        })
        .collect();
    ProdTail {
        from: 16_085,
        cells,
    }
}

/// A topic of the world that no producer has written to: nothing remembered, nothing open.
fn empty_producers(topic: &str, partition: i32, opened: Vec<i64>) -> Option<ProducersView> {
    let head = crate::fixtures_topics::head(topic)?;
    let partitions = i32::try_from(head.parts.len()).unwrap_or(1);
    Some(ProducersView {
        topic: topic.to_owned(),
        partitions,
        partition: partition.clamp(0, partitions - 1),
        now_ms: now(),
        start: 0,
        end: 0,
        last_stable: 0,
        producers: Vec::new(),
        open: Vec::new(),
        aborted: Vec::new(),
        tail: ProdTail {
            from: 0,
            cells: Vec::new(),
        },
        opened,
    })
}

/// A batch of the table: (sequences, offsets, when, in the open transaction).
type Row = ((i32, i32), (i64, i64), &'static str, bool);

fn batches(rows: &[Row]) -> Vec<ProdBatch> {
    rows.iter().map(|r| batch(r.0, r.1, r.2, r.3)).collect()
}

/// The three producers `orders-0` remembers.
fn orders_rows(n: i64) -> Vec<ProdRow> {
    let row = |id, epoch, last, idle_s: i64, txn: Option<ProdTxn>, rows: &[Row]| ProdRow {
        id,
        epoch,
        last_sequence: Some(last),
        last_used_ms: n - idle_s * 1000,
        txn,
        batches: batches(rows),
    };
    let open = ProdTxn {
        id: "orders-eos".to_owned(),
        state: "Ongoing".to_owned(),
        from: Some(16_098),
    };
    vec![
        row(
            4017,
            2,
            22_918,
            4,
            Some(open),
            &[
                (
                    (22_915, 22_918),
                    (16_100, 16_103),
                    "2026-11-09 22:14:21",
                    true,
                ),
                (
                    (22_913, 22_914),
                    (16_098, 16_099),
                    "2026-11-09 22:13:33",
                    true,
                ),
                (
                    (22_909, 22_912),
                    (16_093, 16_096),
                    "2026-11-09 22:13:09",
                    false,
                ),
                (
                    (22_907, 22_908),
                    (16_090, 16_091),
                    "2026-11-09 22:12:40",
                    false,
                ),
                (
                    (22_903, 22_906),
                    (16_085, 16_088),
                    "2026-11-09 22:12:18",
                    false,
                ),
            ],
        ),
        row(
            4023,
            0,
            1140,
            38,
            None,
            &[((1138, 1140), (16_081, 16_083), "2026-11-09 22:13:36", false)],
        ),
        row(
            4031,
            1,
            309,
            3 * 3600 + 12 * 60,
            None,
            &[((308, 309), (16_010, 16_011), "2026-11-09 19:02:14", false)],
        ),
    ]
}

/// The Producers page of `orders` partition 0; `None` for another topic.
#[must_use]
pub fn producers(topic: &str, partition: i32, opened: Vec<i64>) -> Option<ProducersView> {
    if topic != "orders" {
        return empty_producers(topic, partition, opened);
    }
    let n = now();
    Some(ProducersView {
        topic: "orders".to_owned(),
        partitions: 3,
        partition: partition.clamp(0, 2),
        now_ms: n,
        start: 0,
        end: 16_104,
        last_stable: 16_098,
        producers: orders_rows(n),
        open: vec![txn()],
        aborted: vec![
            ProdAborted {
                producer: 4017,
                partition: 0,
                first: 12_877,
                marker: 12_881,
                at_ms: Some(at("2026-11-09 17:14")),
            },
            ProdAborted {
                producer: 3996,
                partition: 1,
                first: 9310,
                marker: 9316,
                at_ms: Some(at("2026-11-08 09:40")),
            },
        ],
        tail: tail(),
        opened,
    })
}

/// The page of the transaction `orders-eos`; `None` for another.
#[must_use]
pub fn txn_view(topic: &str, id: &str) -> Option<TxnView> {
    if topic != "orders" || id != "orders-eos" {
        return None;
    }
    Some(TxnView {
        topic: "orders".to_owned(),
        partitions: 3,
        txn: txn(),
        now_ms: now(),
        parts: vec![
            TxnPart {
                partition: 0,
                first: Some(16_098),
                start: 0,
                end: 16_104,
                last_stable: 16_098,
            },
            TxnPart {
                partition: 1,
                first: Some(16_050),
                start: 0,
                end: 16_051,
                last_stable: 16_050,
            },
        ],
        elsewhere: Vec::new(),
        tail: tail(),
        tail_partition: 0,
        all: vec![txn()],
        more: 0,
    })
}
