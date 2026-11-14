//! The offsets and timestamps of the records of a batch, read without reading what is in them.
//!
//! A v2 record is a length, then an attribute byte, a timestamp delta, an offset delta, and the key, value and
//! headers. The length lets a reader step over what it does not want, so asking when each record was written costs
//! three numbers a record. A compressed batch is walked the same way through its codec, as a stream: what is held is
//! the decoder's window and a small buffer, never the inflated records.
//!
//! The time is the one Kafka gives the record (`DefaultRecordBatch`): the batch's first timestamp plus the record's
//! delta, except in a batch the broker stamped (LogAppendTime), where every record has the batch's newest timestamp,
//! the broker's clock, whatever the producer wrote in the delta.

use std::io::{BufRead, BufReader};

use super::cursor::Cursor;
use super::inflate;
use super::layout::HEADER_LEN;
use super::{Batch, Compression, TimestampType};
use crate::ids::TimestampMs;

/// How much of a compressed stream is read at a time.
const BUFFER: usize = 8 << 10;

/// One record's place and time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordTime {
    /// The record's offset relative to the batch's first.
    pub offset_delta: i32,
    /// When it was written (or, for a batch the broker stamped, when the broker did).
    pub timestamp: TimestampMs,
}

/// The records of a batch, one at a time. Ends at the first record that is not what the header says, with an `Err`.
pub struct RecordTimes<'a> {
    /// `None` once the walk has ended early (or the records could not be opened).
    cursor: Option<Cursor<Box<dyn BufRead + 'a>>>,
    first_timestamp: i64,
    /// The time every record has, in a batch the broker stamped.
    stamped: Option<TimestampMs>,
    left: u32,
}

impl std::fmt::Debug for RecordTimes<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecordTimes")
            .field("open", &self.cursor.is_some())
            .field("first_timestamp", &self.first_timestamp)
            .field("stamped", &self.stamped)
            .field("left", &self.left)
            .finish()
    }
}

impl Batch {
    // fuzz:parser fuzz_batch
    /// The offset delta and timestamp of each record, as Kafka reads them, through the batch's codec. Inflating past
    /// `max_inflated` bytes, a codec this build cannot read, or a record that cannot be read ends the walk with an
    /// `Err`.
    #[must_use]
    pub fn record_times(&self, max_inflated: u64) -> RecordTimes<'_> {
        let body = self.as_bytes().get(HEADER_LEN..).unwrap_or_default();
        let source: Option<Box<dyn BufRead + '_>> = match self.attributes().compression() {
            Compression::None => Some(Box::new(body)),
            codec => inflate::open(codec, body, max_inflated)
                .ok()
                .map(|s| Box::new(BufReader::with_capacity(BUFFER, s)) as Box<dyn BufRead>),
        };
        let stamped = (self.attributes().timestamp_type() == TimestampType::LogAppend)
            .then(|| self.max_timestamp());
        let count = self.record_count();
        RecordTimes {
            // A source that cannot be opened is a walk that fails at its first record.
            left: if source.is_some() {
                count
            } else {
                count.min(1)
            },
            cursor: source.map(|source| Cursor { source, taken: 0 }),
            first_timestamp: self.first_timestamp().get(),
            stamped,
        }
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
            // Nothing past a record that cannot be read can be trusted.
            self.left = 0;
            self.cursor = None;
        }
        Some(read.ok_or(()))
    }
}

impl RecordTimes<'_> {
    fn read_one(&mut self) -> Option<RecordTime> {
        let cursor = self.cursor.as_mut()?;
        let length = u64::try_from(cursor.varint().ok()?).ok()?;
        let end = cursor.taken.checked_add(length)?;
        cursor.byte().ok()?; // the record's attribute byte
        let timestamp_delta = cursor.varint().ok()?;
        let offset_delta = i32::try_from(cursor.varint().ok()?).ok()?;
        // The key, value and headers, stepped over: the record's length says where it ends.
        cursor.skip(end.checked_sub(cursor.taken)?).ok()?;
        let timestamp = match self.stamped {
            Some(at) => at,
            None => TimestampMs::from_wire(self.first_timestamp.checked_add(timestamp_delta)?),
        };
        Some(RecordTime {
            offset_delta,
            timestamp,
        })
    }
}

#[cfg(test)]
mod tests;
