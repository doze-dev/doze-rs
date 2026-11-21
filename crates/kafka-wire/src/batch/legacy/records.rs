//! Every record of a v2 batch, read whole: what a conversion between formats needs, and nothing else in the crate does
//! (the log works from headers, the scan and the compactor step over what they do not want).

use bytes::Bytes;

use super::codec::inflate;
use super::{Magic, Rec};
use crate::batch::inflate::InflateError;
use crate::batch::layout::HEADER_LEN;
use crate::batch::{Batch, Compression, TimestampType, varint};

/// Why a batch's records could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Unread {
    /// They inflate past the limit.
    TooLarge,
    /// They are not what the header says, or not the codec's.
    Corrupt(String),
    /// A codec this build cannot read.
    Unsupported(Compression),
}

impl From<InflateError> for Unread {
    fn from(e: InflateError) -> Self {
        match e {
            InflateError::TooLarge => Self::TooLarge,
            InflateError::Unsupported(c) => Self::Unsupported(c),
            InflateError::Corrupt(why) => Self::Corrupt(why),
        }
    }
}

fn corrupt(what: &str) -> Unread {
    Unread::Corrupt(format!("a record's {what} cannot be read"))
}

/// What every record of a batch takes from its header.
#[derive(Clone, Copy)]
struct Frame {
    base: i64,
    first: i64,
    /// The time every record has, in a batch the broker stamped.
    stamped: Option<i64>,
}

/// The records of `batch`, each with its offset and the time Kafka gives it (a batch the broker stamped gives every
/// record its newest timestamp). At most `limit` bytes are inflated.
pub(super) fn read(batch: &Batch, limit: u64) -> Result<Vec<Rec>, Unread> {
    let body = batch.as_bytes().slice(HEADER_LEN..);
    let records = match batch.attributes().compression() {
        Compression::None => body,
        // The magic only matters to LZ4 of format v0; a v2 batch's frames are standard.
        codec => Bytes::from(inflate(Magic::V1, codec, &body, limit)?),
    };
    let frame = Frame {
        base: batch.base_offset().map_or(0, |o| o.get()),
        first: batch.first_timestamp().get(),
        stamped: (batch.attributes().timestamp_type() == TimestampType::LogAppend)
            .then(|| batch.max_timestamp().get()),
    };
    let count = usize::try_from(batch.record_count()).unwrap_or(0);
    let mut out = Vec::with_capacity(count.min(1024));
    let mut at = 0_usize;
    for _ in 0..count {
        let mut rest = records.get(at..).ok_or_else(|| corrupt("length"))?;
        let before = rest.len();
        let length = varint::read(&mut rest)
            .and_then(|l| usize::try_from(l).ok())
            .ok_or_else(|| corrupt("length"))?;
        at += before - rest.len();
        let end = at
            .checked_add(length)
            .filter(|e| *e <= records.len())
            .ok_or_else(|| corrupt("body"))?;
        out.push(one(&records.slice(at..end), frame)?);
        at = end;
    }
    Ok(out)
}

/// One record's body (after its length).
fn one(record: &Bytes, frame: Frame) -> Result<Rec, Unread> {
    let mut r: &[u8] = record.get(1..).ok_or_else(|| corrupt("attributes"))?;
    let delta = varint::read(&mut r).ok_or_else(|| corrupt("timestamp"))?;
    let offset_delta = varint::read(&mut r).ok_or_else(|| corrupt("offset"))?;
    let key = field(record, &mut r)?;
    let value = field(record, &mut r)?;
    let count = varint::read(&mut r).ok_or_else(|| corrupt("header count"))?;
    let mut headers = Vec::new();
    for _ in 0..count.max(0) {
        let name = field(record, &mut r)?.unwrap_or_default();
        let value = field(record, &mut r)?.unwrap_or_default();
        headers.push((name, value));
    }
    Ok(Rec {
        offset: frame.base.saturating_add(offset_delta),
        timestamp: frame
            .stamped
            .unwrap_or_else(|| frame.first.saturating_add(delta)),
        key,
        value,
        headers,
    })
}

/// A length-prefixed field at the front of `r` (a part of `record`), as a slice of `record`; `None` for length -1.
fn field(record: &Bytes, r: &mut &[u8]) -> Result<Option<Bytes>, Unread> {
    let len = varint::read(r).ok_or_else(|| corrupt("field length"))?;
    if len < 0 {
        return Ok(None);
    }
    let len = usize::try_from(len).map_err(|_| corrupt("field length"))?;
    let after = r.get(len..).ok_or_else(|| corrupt("field"))?;
    let from = record.len() - r.len();
    *r = after;
    Ok(Some(record.slice(from..from + len)))
}
