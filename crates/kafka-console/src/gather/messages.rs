//! The Messages page: which records to read for the filters asked, newest first.
//!
//! - Nothing asked: the last records of every partition (or the chosen one).
//! - From an offset: forward from it, on the chosen partition.
//! - From a time: ListOffsets finds the first record at or after it on each partition, then forward from there.
//! - A key: the partition is searched backwards from its end, a thousand offsets at a time, until enough matched or 8 MiB
//!   of records have been looked at.

use kafka_wire::api::Response;

use super::decode::shown;
use super::meta::{self, TopicMeta};
use super::read::{self, MOST_BYTES};
use super::topic;
use super::walk::{Keep, Raw};
use crate::backend::{Backend, requests};
use crate::model::{
    Hold, MessageQuery, MessagesView, Reach, Reading, RecordRow, Standing, TopicHead,
};

/// What a table keeps of each record: its key and the start of its value are enough for a cell.
const TABLE: Keep = Keep {
    key: 4096,
    value: 512,
    header: 0,
};
/// How many offsets a key search looks at in one window.
const WINDOW: i64 = 1000;

/// The row of the table for a record.
fn row(r: &Raw) -> RecordRow {
    RecordRow {
        partition: r.partition,
        offset: r.offset,
        timestamp_ms: r.timestamp,
        key: shown(r.key.as_deref(), r.key_len),
        value: shown(r.value.as_deref(), r.value_len),
        standing: r.standing,
    }
}

/// Where each partition stops for the reading: the log end, or the last stable offset for `read_committed`.
fn stop(head: &TopicHead, reading: Reading, partition: i32) -> (i64, i64) {
    head.parts
        .iter()
        .find(|p| p.partition == partition)
        .map_or((0, 0), |p| {
            let end = if reading == Reading::Committed {
                p.last_stable
            } else {
                p.end
            };
            (p.start, end)
        })
}

/// Records found, and what was done to find them.
#[derive(Default)]
struct Found {
    rows: Vec<Raw>,
    reached_end: bool,
    scanned: Option<(u64, bool)>,
    error: Option<i16>,
}

async fn latest(
    b: &Backend,
    t: &TopicMeta,
    head: &TopicHead,
    parts: &[i32],
    q: &MessageQuery,
) -> Found {
    let mut out = Found {
        reached_end: true,
        ..Found::default()
    };
    let n = i64::try_from(q.limit).unwrap_or(25);
    for &p in parts {
        let (start, end) = stop(head, q.reading, p);
        let mut from = (end - n).max(start);
        loop {
            let w = read::forward(b, t, (p, from, end), q.reading, (TABLE, None)).await;
            out.error = out.error.or(w.error);
            out.reached_end &= w.reached;
            let enough = w.rows.len() >= q.limit || from <= start;
            if enough {
                out.rows.extend(w.rows);
                break;
            }
            from = (from - 2 * n).max(start);
        }
    }
    out
}

async fn from_offsets(
    b: &Backend,
    t: &TopicMeta,
    head: &TopicHead,
    starts: &[(i32, i64)],
    q: &MessageQuery,
) -> Found {
    let mut out = Found {
        reached_end: true,
        ..Found::default()
    };
    for &(p, from) in starts {
        let (start, end) = stop(head, q.reading, p);
        let w = read::forward(
            b,
            t,
            (p, from.max(start), end),
            q.reading,
            (TABLE, Some(q.limit)),
        )
        .await;
        out.error = out.error.or(w.error);
        out.reached_end &= w.reached && w.rows.len() <= q.limit;
        // The oldest first: what was asked for is the start, so the cut is at the newest end.
        let mut rows = w.rows;
        rows.sort_by_key(|r| r.offset);
        rows.truncate(q.limit);
        out.rows.extend(rows);
    }
    out
}

/// The first offset at or after `ms` on each of `parts`.
async fn offsets_at(
    b: &Backend,
    topic: &str,
    parts: &[i32],
    ms: i64,
    reading: Reading,
) -> Vec<(i32, i64)> {
    use requests::{Ask, Isolation};
    let asks: Vec<Ask> = parts
        .iter()
        .map(|&p| Ask {
            partition: p,
            timestamp: ms,
        })
        .collect();
    let iso = if reading == Reading::Committed {
        Isolation::ReadCommitted
    } else {
        Isolation::ReadUncommitted
    };
    let Ok(Response::ListOffsets(r)) = b.call(requests::list_offsets(topic, &asks, iso)).await
    else {
        return Vec::new();
    };
    r.topics
        .first()
        .map(|t| {
            t.partitions
                .iter()
                .filter(|p| p.error_code == 0 && p.offset >= 0)
                .map(|p| (p.partition_index, p.offset))
                .collect()
        })
        .unwrap_or_default()
}

async fn by_key(
    b: &Backend,
    t: &TopicMeta,
    head: &TopicHead,
    parts: &[i32],
    q: &MessageQuery,
) -> Found {
    let wanted = q.key.as_deref().unwrap_or_default().as_bytes();
    let mut out = Found {
        reached_end: true,
        scanned: Some((0, true)),
        ..Found::default()
    };
    let (mut looked, mut complete, mut bytes) = (0_u64, true, 0_usize);
    for &p in parts {
        let (start, end) = stop(head, q.reading, p);
        let mut to = end;
        while to > start {
            if bytes >= MOST_BYTES || out.rows.len() >= q.limit {
                complete &= to <= start;
                break;
            }
            let from = (to - WINDOW).max(start);
            let w = read::forward(b, t, (p, from, to), q.reading, (TABLE, None)).await;
            out.error = out.error.or(w.error);
            bytes += w.bytes;
            looked += w.rows.len() as u64;
            out.rows.extend(
                w.rows.into_iter().filter(|r| {
                    r.key.as_deref() == Some(wanted) && r.key_len == wanted.len() as u64
                }),
            );
            to = from;
        }
    }
    out.scanned = Some((looked, complete));
    out
}

fn holds(head: &TopicHead) -> Vec<Hold> {
    head.parts
        .iter()
        .map(|p| Hold {
            partition: p.partition,
            high_watermark: p.end,
            last_stable: p.last_stable,
            txn: head.txns.iter().find_map(|t| {
                t.partitions
                    .iter()
                    .find(|(tp, first)| *tp == p.partition && first.is_some())
                    .and_then(|(_, first)| first.map(|f| (t.id.clone(), f)))
            }),
        })
        .collect()
}

fn reach(rows: &[RecordRow], f: &Found, more: bool) -> Reach {
    Reach {
        returned: rows.len(),
        first_offset: rows.iter().map(|r| r.offset).min(),
        last_offset: rows.iter().map(|r| r.offset).max(),
        reached_end: f.reached_end,
        above_lso: rows
            .iter()
            .filter(|r| r.standing == Standing::AboveLso)
            .count(),
        scanned: f.scanned,
        more,
    }
}

/// Newest first: by time across partitions, by offset within one.
fn newest_first(rows: &mut [Raw], single: bool) {
    if single {
        rows.sort_by_key(|r| std::cmp::Reverse(r.offset));
    } else {
        rows.sort_by_key(|r| {
            (
                std::cmp::Reverse(r.timestamp),
                r.partition,
                std::cmp::Reverse(r.offset),
            )
        });
    }
}

/// The page for `query` on topic `name`; `None` if the topic does not exist (or may not be described).
pub async fn page(
    b: &Backend,
    name: &str,
    query: MessageQuery,
    mut notes: Vec<String>,
) -> Option<MessagesView> {
    let meta = meta::one(b, name).await?;
    let head = topic::head_of(b, &meta).await;
    let mut q = query;
    let missing: Vec<String> = q
        .partitions
        .iter()
        .filter(|p| !meta.partitions.contains(p))
        .map(ToString::to_string)
        .collect();
    if !missing.is_empty() {
        notes.push(format!(
            "{} has no partition {}, so {} left out.",
            head.name,
            missing.join(", "),
            if missing.len() == 1 {
                "it is"
            } else {
                "they are"
            }
        ));
        q.partitions.retain(|p| meta.partitions.contains(p));
    }
    let parts: Vec<i32> = if q.partitions.is_empty() {
        meta.partitions.clone()
    } else {
        q.partitions.clone()
    };
    if q.offset.is_some() && parts.len() != 1 {
        notes.push(
            "An offset belongs to one partition: choose a partition and the offset is used."
                .to_owned(),
        );
        q.offset = None;
    }
    let mut found = search(b, &meta, &head, &parts, &q).await;
    if let Some(code) = found.error.filter(|c| *c != 0) {
        notes.push(crate::words::read_refused(code));
    }
    newest_first(&mut found.rows, parts.len() == 1);
    let more = found.rows.len() > q.limit
        || (!q.filtered()
            && head.records() > i64::try_from(found.rows.len().min(q.limit)).unwrap_or(0));
    found.rows.truncate(q.limit);
    let rows: Vec<RecordRow> = found.rows.iter().map(row).collect();
    let reach = reach(&rows, &found, more);
    Some(MessagesView {
        holds: holds(&head),
        head,
        query: q,
        notes,
        rows,
        reach,
    })
}

async fn search(
    b: &Backend,
    meta: &TopicMeta,
    head: &TopicHead,
    parts: &[i32],
    q: &MessageQuery,
) -> Found {
    if q.key.is_some() {
        return by_key(b, meta, head, parts, q).await;
    }
    if let Some(offset) = q.offset {
        let starts: Vec<(i32, i64)> = parts.iter().map(|&p| (p, offset)).collect();
        return from_offsets(b, meta, head, &starts, q).await;
    }
    if let Some((ms, _)) = &q.time {
        let starts = offsets_at(b, &meta.name, parts, *ms, q.reading).await;
        return from_offsets(b, meta, head, &starts, q).await;
    }
    latest(b, meta, head, parts, q).await
}
