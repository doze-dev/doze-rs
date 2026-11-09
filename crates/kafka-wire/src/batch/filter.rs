//! Taking records out of a batch, as compaction does: the ones kept stay exactly as they were (their offsets, so the log
//! has gaps), the batch keeps its place and the offsets it spans (so the log's end and a producer's last sequence are not
//! lost), and a batch with a tombstone or a marker that is to go in time says when (the delete horizon).

use std::io::Read;

use bytes::Bytes;

use super::attributes::Attributes;
use super::inflate::{self, InflateError};
use super::layout::{
    ATTRIBUTES, CRC, FIRST_TIMESTAMP, HEADER_LEN, LENGTH, LENGTH_OVERHEAD, MAX_TIMESTAMP,
    RECORD_COUNT, crc_of,
};
use super::recode::{pack, put, read_varint, write_varint};
use super::scan::RecordsRefused;
use super::{Batch, Compression, Refusal};
use crate::ids::Offset;

/// A record as compaction judges it.
#[derive(Debug, Clone, Copy)]
pub struct Seen<'a> {
    /// Its offset in the log.
    pub offset: i64,
    /// Its key, if it has one.
    pub key: Option<&'a [u8]>,
    /// It has no value: a deletion.
    pub tombstone: bool,
}

/// What a filtering is told besides which records to keep.
#[derive(Debug, Clone, Copy)]
pub struct Rules {
    /// The most the batch may inflate to.
    pub max_inflated: u64,
    /// The time to give a batch that keeps a tombstone (or, a marker for a transaction with nothing left) when it has
    /// none yet: now plus how long deletions are kept.
    pub horizon: i64,
    /// This batch is a transaction marker whose transaction has no records left: it is to be given a horizon.
    pub marker_of_empty_transaction: bool,
}

/// A batch after filtering.
#[derive(Debug, Clone)]
pub struct Filtered {
    /// The batch: the original if nothing changed, else the same batch with only the records kept (none, possibly).
    pub batch: Batch,
    /// How many records it holds now.
    pub kept: u32,
    /// Whether it differs from the original.
    pub changed: bool,
}

impl Batch {
    /// When the tombstones and markers of the batch may be removed, if compaction has set it.
    #[must_use]
    pub fn delete_horizon(&self) -> Option<i64> {
        self.attributes()
            .has_delete_horizon()
            .then(|| self.first_timestamp().get())
    }

    // fuzz:parser fuzz_batch
    /// The batch with only the records `keep` says yes to. Each keeps its offset, timestamp and everything else; the
    /// batch keeps its base offset, its span, its producer and its sequence. If it keeps a tombstone, or is a marker
    /// that `rules` says is to be given a horizon, and has none, it is given `rules.horizon`: the first timestamp
    /// becomes the horizon, and the records' timestamp deltas are recomputed against it.
    ///
    /// # Errors
    /// A records section that cannot be read, one that inflates past the limit, or a codec this build cannot write.
    pub fn filter(
        &self,
        rules: Rules,
        mut keep: impl FnMut(&Seen<'_>) -> bool,
    ) -> Result<Filtered, RecordsRefused> {
        let unreadable =
            |why: &str| RecordsRefused::new(Refusal::UnknownServerError, false, why.to_owned());
        let compression = self.attributes().compression();
        let inflated = self.inflated(rules.max_inflated)?;
        let first = self.first_timestamp().get();
        let base = self.base_offset().map_or(0, Offset::get);
        let mut kept: Vec<(i64, &[u8])> = Vec::new();
        let (mut rest, mut tombstone) = (inflated.as_slice(), false);
        for _ in 0..self.record_count() {
            let (record, after) =
                split_record(rest).ok_or_else(|| unreadable("a record is cut short"))?;
            rest = after;
            let parsed = parse(record).ok_or_else(|| unreadable("a record cannot be read"))?;
            let seen = Seen {
                offset: base.saturating_add(parsed.offset_delta),
                key: parsed.key,
                tombstone: parsed.tombstone,
            };
            if keep(&seen) {
                tombstone |= parsed.tombstone;
                kept.push((first.saturating_add(parsed.timestamp_delta), record));
            }
        }
        let count = u32::try_from(kept.len()).unwrap_or(u32::MAX);
        let horizon_wanted = self.delete_horizon().is_none()
            && count > 0
            && (tombstone || (self.attributes().is_control() && rules.marker_of_empty_transaction));
        if count == self.record_count() && !horizon_wanted {
            return Ok(Filtered {
                batch: self.clone(),
                kept: count,
                changed: false,
            });
        }
        let new_base_timestamp = if horizon_wanted { rules.horizon } else { first };
        let mut records = Vec::with_capacity(inflated.len());
        for (timestamp, record) in &kept {
            if horizon_wanted {
                retimed(&mut records, record, timestamp - new_base_timestamp)
                    .ok_or_else(|| unreadable("a record cannot be written again"))?;
            } else {
                write_varint(&mut records, i64::try_from(record.len()).unwrap_or(0));
                records.extend_from_slice(record);
            }
        }
        self.rebuilt(
            &kept,
            count,
            horizon_wanted.then_some(new_base_timestamp),
            &records,
            compression,
        )
    }

    /// Calls `look` with each record of the batch, in order.
    ///
    /// # Errors
    /// A records section that cannot be read, or one that inflates past `max_inflated`.
    pub fn each_record(
        &self,
        max_inflated: u64,
        mut look: impl FnMut(&Seen<'_>),
    ) -> Result<(), RecordsRefused> {
        let unreadable =
            |why: &str| RecordsRefused::new(Refusal::UnknownServerError, false, why.to_owned());
        let inflated = self.inflated(max_inflated)?;
        let base = self.base_offset().map_or(0, Offset::get);
        let mut rest = inflated.as_slice();
        for _ in 0..self.record_count() {
            let (record, after) =
                split_record(rest).ok_or_else(|| unreadable("a record is cut short"))?;
            rest = after;
            let p = parse(record).ok_or_else(|| unreadable("a record cannot be read"))?;
            look(&Seen {
                offset: base.saturating_add(p.offset_delta),
                key: p.key,
                tombstone: p.tombstone,
            });
        }
        Ok(())
    }

    /// The records section, inflated.
    fn inflated(&self, max_inflated: u64) -> Result<Vec<u8>, RecordsRefused> {
        let body = self.as_bytes().get(HEADER_LEN..).unwrap_or_default();
        let mut inflated = Vec::new();
        inflate::open(self.attributes().compression(), body, max_inflated)
            .and_then(|mut s| s.read_to_end(&mut inflated).map_err(|e| inflate::why(&e)))
            .map_err(|e| match e {
                InflateError::TooLarge => RecordsRefused::new(
                    Refusal::MessageTooLarge,
                    true,
                    "the records inflate past the limit",
                ),
                other => RecordsRefused::new(
                    Refusal::UnknownServerError,
                    false,
                    format!("the records cannot be read: {other:?}"),
                ),
            })?;
        Ok(inflated)
    }

    fn rebuilt(
        &self,
        kept: &[(i64, &[u8])],
        count: u32,
        horizon: Option<i64>,
        records: &[u8],
        compression: Compression,
    ) -> Result<Filtered, RecordsRefused> {
        let failed = |why: String| RecordsRefused::new(Refusal::UnknownServerError, false, why);
        // A batch with nothing in it is not compressed (there is nothing to read from it).
        let codec = if count == 0 {
            Compression::None
        } else {
            compression
        };
        let packed = pack(codec, records)?;
        let mut out = Vec::with_capacity(HEADER_LEN + packed.len());
        out.extend_from_slice(self.as_bytes().get(..HEADER_LEN).unwrap_or_default());
        out.extend_from_slice(&packed);
        let length = u32::try_from(out.len() - usize::try_from(LENGTH_OVERHEAD).unwrap_or(12))
            .map_err(|_| failed("the batch is too long to write".to_owned()))?;
        let mut attributes: Attributes = self.attributes();
        if count == 0 {
            attributes = attributes.uncompressed();
        }
        if horizon.is_some() {
            attributes = attributes.with_delete_horizon();
        }
        put(&mut out, LENGTH, &length.to_be_bytes());
        put(&mut out, ATTRIBUTES, &attributes.bits().to_be_bytes());
        put(
            &mut out,
            RECORD_COUNT,
            &i32::try_from(count).unwrap_or(0).to_be_bytes(),
        );
        if let Some(h) = horizon {
            put(&mut out, FIRST_TIMESTAMP, &h.to_be_bytes());
        }
        if let Some(newest) = kept.iter().map(|(t, _)| *t).max() {
            put(&mut out, MAX_TIMESTAMP, &newest.to_be_bytes());
        }
        let crc = crc_of(&out);
        put(&mut out, CRC, &crc.to_be_bytes());
        let batch = Batch::parse_stored(Bytes::from(out))
            .map_err(|e| failed(format!("the batch written again is not one: {e}")))?;
        Ok(Filtered {
            batch,
            kept: count,
            changed: true,
        })
    }
}

/// One record's fields that compaction looks at.
struct Parsed<'a> {
    timestamp_delta: i64,
    offset_delta: i64,
    key: Option<&'a [u8]>,
    tombstone: bool,
}

/// The first record of `bytes` (without its length prefix) and what follows it.
fn split_record(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let (length, used) = read_varint(bytes)?;
    let end = used.checked_add(usize::try_from(length).ok()?)?;
    Some((bytes.get(used..end)?, bytes.get(end..)?))
}

fn parse(record: &[u8]) -> Option<Parsed<'_>> {
    let rest = record.get(1..)?; // the record's attribute byte
    let (timestamp_delta, n) = read_varint(rest)?;
    let rest = rest.get(n..)?;
    let (offset_delta, n) = read_varint(rest)?;
    let rest = rest.get(n..)?;
    let (key_len, n) = read_varint(rest)?;
    let rest = rest.get(n..)?;
    let (key, rest) = if key_len < 0 {
        (None, rest)
    } else {
        let len = usize::try_from(key_len).ok()?;
        (Some(rest.get(..len)?), rest.get(len..)?)
    };
    let (value_len, _) = read_varint(rest)?;
    Some(Parsed {
        timestamp_delta,
        offset_delta,
        key,
        tombstone: value_len < 0,
    })
}

/// `record` written to `out` with its timestamp delta replaced.
fn retimed(out: &mut Vec<u8>, record: &[u8], delta: i64) -> Option<()> {
    let (attributes, rest) = record.split_first()?;
    let (_, n) = read_varint(rest)?;
    let mut body = vec![*attributes];
    write_varint(&mut body, delta);
    body.extend_from_slice(rest.get(n..)?);
    write_varint(out, i64::try_from(body.len()).ok()?);
    out.extend_from_slice(&body);
    Some(())
}

#[cfg(test)]
mod tests;
