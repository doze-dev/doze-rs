//! The records of a Fetch answer, read for display: every record of every batch, in order, with where each stands for a
//! `read_committed` consumer. The same bookkeeping a consumer does (Kafka's `Fetcher`): an aborted transaction's records
//! are skipped from its first offset until its abort marker.

use std::collections::BTreeSet;

use bytes::Bytes;
use kafka_wire::batch::{Batch, Batches, Compression, TimestampType};

use crate::model::Standing;

/// The most a batch is allowed to inflate to: the broker's own limit when it accepted the batch.
pub const INFLATE_CAP: u64 = 64 << 20;

/// How much of a key, a value and a header a [`Raw`] keeps (the rest is only counted).
#[derive(Debug, Clone, Copy)]
pub struct Keep {
    pub key: usize,
    pub value: usize,
    pub header: usize,
}

/// A record, with the first bytes of its key, value and headers and their true sizes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raw {
    pub partition: i32,
    pub offset: i64,
    pub timestamp: i64,
    pub key: Option<Vec<u8>>,
    pub key_len: u64,
    pub value: Option<Vec<u8>>,
    pub value_len: u64,
    pub headers: Vec<(Vec<u8>, Option<Vec<u8>>, u64)>,
    /// Producer id, epoch and the record's own sequence number, for a batch from an idempotent producer.
    pub producer: Option<(i64, i16, i64)>,
    pub transactional: bool,
    pub log_append: bool,
    pub codec: &'static str,
    pub standing: Standing,
}

/// What walking an answer found.
#[derive(Debug, Default)]
pub struct Walked {
    /// The records at or after the offset asked for, in log order.
    pub rows: Vec<Raw>,
    /// A batch of a transaction was met (so the aborted list matters).
    pub saw_txn: bool,
    /// The offset after the last batch read (where the next fetch begins), if any batch was read.
    pub next: Option<i64>,
}

fn codec_name(c: Compression) -> &'static str {
    match c {
        Compression::None => "none",
        Compression::Gzip => "gzip",
        Compression::Snappy => "snappy",
        Compression::Lz4 => "lz4",
        Compression::Zstd => "zstd",
        _ => "unknown",
    }
}

fn kept(field: Option<&[u8]>, n: usize) -> (Option<Vec<u8>>, u64) {
    (
        field.map(|f| f.get(..f.len().min(n)).unwrap_or_default().to_vec()),
        field.map_or(0, |f| f.len() as u64),
    )
}

/// Where the transactions stand while batches are walked in order.
struct Aborts {
    /// `(first offset, producer id)` of the aborted transactions, oldest first, not yet reached.
    pending: Vec<(i64, i64)>,
    /// Producers whose records are aborted right now.
    active: BTreeSet<i64>,
}

impl Aborts {
    fn new(aborted: &[(i64, i64)]) -> Self {
        let mut pending = aborted.to_vec();
        pending.sort_unstable();
        pending.reverse();
        Self { pending, active: BTreeSet::new() }
    }

    /// Called at the start of each batch.
    fn reach(&mut self, base: i64) {
        while let Some(&(first, producer)) = self.pending.last() {
            if first > base {
                break;
            }
            self.active.insert(producer);
            self.pending.pop();
        }
    }
}

fn standing(offset: i64, lso: i64, aborted: bool) -> Standing {
    if offset >= lso {
        Standing::AboveLso
    } else if aborted {
        Standing::Aborted
    } else {
        Standing::Committed
    }
}

/// Walks the batches of `data` (a partition's `records` of a Fetch answer). `from` is the offset asked for: records before it
/// (the batch that holds it may begin earlier) are not returned. `lso` is the last stable offset and `aborted` the answer's
/// `(producer id, first offset)` pairs; with none given, nothing is aborted. At most `limit` records are returned.
#[must_use]
pub fn walk(
    data: &Bytes,
    partition: i32,
    from: i64,
    lso: i64,
    aborted: &[(i64, i64)],
    (keep, limit): (Keep, usize),
) -> Walked {
    let mut out = Walked::default();
    let mut aborts = Aborts::new(&aborted.iter().map(|(p, f)| (*f, *p)).collect::<Vec<_>>());
    for batch in Batches::new(data.clone()) {
        let Ok(batch) = batch else {
            break;
        };
        out.next = batch.next_offset().map(|o| o.get());
        let base = batch.base_offset().map_or(0, |o| o.get());
        aborts.reach(base);
        let attrs = batch.attributes();
        if attrs.is_control() {
            if let Some(p) = batch.producer() {
                aborts.active.remove(&p.id.get());
            }
            continue;
        }
        out.saw_txn |= attrs.is_transactional();
        let hidden = attrs.is_transactional()
            && batch.producer().is_some_and(|p| aborts.active.contains(&p.id.get()));
        batch_rows(&batch, partition, from, (lso, hidden), keep, &mut out.rows, limit);
        if out.rows.len() >= limit {
            break;
        }
    }
    out
}

fn batch_rows(
    batch: &Batch,
    partition: i32,
    from: i64,
    (lso, aborted): (i64, bool),
    keep: Keep,
    rows: &mut Vec<Raw>,
    limit: usize,
) {
    let attrs = batch.attributes();
    let producer = batch.producer();
    let base_offset = batch.base_offset().map_or(0, |o| o.get());
    let codec = codec_name(attrs.compression());
    let log_append = attrs.timestamp_type() == TimestampType::LogAppend;
    let transactional = attrs.is_transactional();
    // Errors end the batch: what was read before stays.
    let _ = batch.each_record_full(INFLATE_CAP, |r| {
        if r.offset < from {
            return true;
        }
        let (key, key_len) = kept(r.key, keep.key);
        let (value, value_len) = kept(r.value, keep.value);
        rows.push(Raw {
            partition,
            offset: r.offset,
            timestamp: r.timestamp,
            key,
            key_len,
            value,
            value_len,
            headers: r
                .headers
                .iter()
                .take(64)
                .map(|(k, v)| {
                    let (v, n) = kept(*v, keep.header);
                    (k.get(..k.len().min(1024)).unwrap_or_default().to_vec(), v, n)
                })
                .collect(),
            producer: producer.map(|p| {
                (
                    p.id.get(),
                    p.epoch.get(),
                    p.base_sequence
                        .map_or(-1, |s| i64::from(s) + (r.offset - base_offset)),
                )
            }),
            transactional,
            log_append,
            codec,
            standing: standing(r.offset, lso, aborted),
        });
        rows.len() < limit
    });
}

#[cfg(test)]
mod tests;
