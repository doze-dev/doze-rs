//! The Producers and transaction boards of the larder: `orders` with its open transaction `orders-eos`.

use super::{at, now};
use crate::model::{
    ProdAborted, ProdBatch, ProdCell, ProdRow, ProdTail, ProdTxn, ProducersView, TxnPart, TxnRow,
    TxnView,
};

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
