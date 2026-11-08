//! The offsets and timestamps of the records of a batch, read without reading what is in them.
//!
//! A v2 record is a length, then an attribute byte, a timestamp delta, an offset delta, and the key, value and
//! headers. The length lets a reader step over what it does not want, so asking when each record was written costs
//! three numbers a record. Only an uncompressed batch can be walked this way: the records of a compressed one are
//! inside the codec (see `docs/port/phase3.md`, slice 3f).

use super::layout::HEADER_LEN;
use super::{Batch, Compression};
use crate::ids::TimestampMs;

/// One record's place and time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordTime {
    /// The record's offset relative to the batch's first.
    pub offset_delta: i32,
    /// When it was written (or, for a batch the broker stamped, when the broker did).
    pub timestamp: TimestampMs,
}

/// The records of an uncompressed batch, one at a time. Ends at the first record that is not what the header says.
#[derive(Debug, Clone)]
pub struct RecordTimes<'a> {
    rest: &'a [u8],
    first_timestamp: i64,
    left: u32,
}

impl Batch {
    // fuzz:parser fuzz_batch
    /// The offset delta and timestamp of each record, or `None` for a compressed batch, whose records cannot be
    /// walked without inflating them. A record that cannot be read ends the walk with an `Err`.
    #[must_use]
    pub fn record_times(&self) -> Option<RecordTimes<'_>> {
        if self.attributes().compression() != Compression::None {
            return None;
        }
        Some(RecordTimes {
            rest: self.as_bytes().get(HEADER_LEN..)?,
            first_timestamp: self.first_timestamp().get(),
            left: self.record_count(),
        })
    }
}

impl Iterator for RecordTimes<'_> {
    type Item = Result<RecordTime, ()>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.left == 0 {
            return None;
        }
        self.left -= 1;
        let read = self.read_one();
        if read.is_none() {
            self.left = 0; // nothing past a record that cannot be read can be trusted
        }
        Some(read.ok_or(()))
    }
}

impl RecordTimes<'_> {
    fn read_one(&mut self) -> Option<RecordTime> {
        let length = usize::try_from(varint(&mut self.rest)?).ok()?;
        let (mut record, after) = (self.rest.get(..length)?, self.rest.get(length..)?);
        self.rest = after;
        record = record.get(1..)?; // the record's attribute byte
        let timestamp_delta = varint(&mut record)?;
        let offset_delta = i32::try_from(varint(&mut record)?).ok()?;
        Some(RecordTime {
            offset_delta,
            timestamp: TimestampMs::from_wire(self.first_timestamp.checked_add(timestamp_delta)?),
        })
    }
}

/// A zigzag varint (Kafka's `varint` and `varlong`) off the front of `bytes`.
fn varint(bytes: &mut &[u8]) -> Option<i64> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let (&byte, rest) = bytes.split_first()?;
        *bytes = rest;
        value |= u64::from(byte & 0x7f).checked_shl(shift)?;
        if byte & 0x80 == 0 {
            // Zigzag: the low bit is the sign.
            return Some(((value >> 1) as i64) ^ -((value & 1) as i64));
        }
    }
    None // more than ten bytes is no number
}

#[cfg(test)]
mod tests;
