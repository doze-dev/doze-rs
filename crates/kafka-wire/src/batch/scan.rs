//! Reading every record of a batch before it is stored: the count the header gives is the count there is, every
//! record is exactly as long as it says, a topic that compacts has a key on each, and a CreateTime topic's
//! timestamps are within its bounds of the clock. Records are read through whatever codec the batch uses, as a
//! stream: nothing inflated is held.
//!
//! Kafka's answers to these are recorded (`produce-validation`), including the words of two of them.

use std::fmt::Display;
use std::io::{BufRead, BufReader};

mod cursor;
mod judge;

use cursor::{Cursor, Fault, Stop};
use judge::Judge;

use super::inflate::{self, InflateError};
use super::{Batch, Compression, Refusal};
use crate::ids::TimestampMs;

/// How much of a compressed stream is read at a time.
const BUFFER: usize = 32 << 10;

/// What a topic and the clock add to the rules every batch is held to.
#[derive(Debug, Clone)]
pub struct ScanRules {
    /// A record with no key is refused (a topic that compacts).
    pub require_keys: bool,
    /// The topic stamps records with the broker's clock, so a producer's timestamps are not held to it.
    pub log_append_time: bool,
    /// The broker's clock.
    pub now: TimestampMs,
    /// How far behind `now` a CreateTime record may be stamped.
    pub before_max_ms: i64,
    /// How far ahead of `now`.
    pub after_max_ms: i64,
    /// The most a batch may inflate to.
    pub max_inflated: u64,
}

/// What a scan found out about a batch that passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scanned {
    /// The latest of the records' timestamps (a producer may write anything in the header's), or -1 if none has one.
    pub max_timestamp: TimestampMs,
}

/// One record's fault, as the answer lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordError {
    /// Which record of the batch.
    pub index: i32,
    /// What is wrong with it.
    pub message: String,
}

/// Why the records of a batch are refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordsRefused {
    /// How Kafka answers it.
    pub refusal: Refusal,
    /// Whether Kafka refuses it before it has asked the partition anything (the answer then has no log start).
    pub before_log: bool,
    /// The text Kafka's answer carries, where it has one.
    pub message: Option<String>,
    /// The records at fault, where it is some of them.
    pub records: Vec<RecordError>,
    /// What is wrong, in this broker's words (for its log).
    pub reason: String,
}

impl RecordsRefused {
    fn new(refusal: Refusal, before_log: bool, reason: impl Into<String>) -> Self {
        Self {
            refusal,
            before_log,
            message: None,
            records: Vec::new(),
            reason: reason.into(),
        }
    }

    fn invalid(reason: impl Into<String>) -> Self {
        Self::new(Refusal::InvalidRecord, false, reason)
    }

    fn inflating(e: InflateError, batch_bytes: usize) -> Self {
        match e {
            InflateError::TooLarge => Self::new(
                Refusal::MessageTooLarge,
                true,
                format!(
                    "a record batch of {batch_bytes} bytes inflates to more than this broker will inflate"
                ),
            ),
            InflateError::Unsupported(codec) => Self::new(
                Refusal::UnsupportedCompression,
                true,
                format!("this build has no decoder for {codec:?}"),
            ),
            InflateError::Corrupt(why) => Self::new(
                Refusal::UnknownServerError,
                false,
                format!("the records cannot be decompressed: {why}"),
            ),
        }
    }
}

/// What one record said of itself.
struct Record {
    timestamp_delta: i64,
    offset_delta: i64,
    key_length: i64,
}

/// Reads one record's fields inside the length it gives itself: nothing may be read past where it stops.
fn read_record<R: BufRead>(
    cursor: &mut Cursor<R>,
    index: i32,
    count: i32,
) -> Result<Record, Fault> {
    let fault = |what: &str| Fault::Malformed(format!("record {index} of {count} {what}"));
    let cut = |e: Stop, what: &str| match e {
        Stop::Source(e) => Fault::Source(e),
        _ => fault(what),
    };
    let length = cursor.varint().map_err(|e| cut(e, "has no length"))?;
    if length < 0 {
        return Err(fault("has no length"));
    }
    let start = cursor.taken;
    let end = start + length as u64;
    cursor.byte().map_err(|e| cut(e, "is cut short"))?; // attributes: unused
    let timestamp_delta = cursor.varint().map_err(|e| cut(e, "is cut short"))?;
    let offset_delta = cursor.varint().map_err(|e| cut(e, "is cut short"))?;
    // A negative length is a null key or value, whatever the number (Kafka reads any as null; a client writes -1).
    let key_length = cursor.varint().map_err(|e| cut(e, "is cut short"))?;
    skip_field(cursor, key_length, end)
        .map_err(|e| cut(e, "has a key that runs past the record"))?;
    let value_length = cursor.varint().map_err(|e| cut(e, "is cut short"))?;
    skip_field(cursor, value_length, end)
        .map_err(|e| cut(e, "has a value that runs past the record"))?;
    skip_headers(cursor, end).map_err(|e| cut(e, "has a header that cannot be"))?;
    if cursor.taken != end {
        return Err(fault(&format!(
            "says it is {length} bytes long and its fields are {}",
            cursor.taken - start
        )));
    }
    Ok(Record {
        timestamp_delta,
        offset_delta,
        key_length,
    })
}

/// Steps over a field of `length` bytes (none if negative) that must end by `end`.
fn skip_field<R: BufRead>(cursor: &mut Cursor<R>, length: i64, end: u64) -> Result<(), Stop> {
    let Ok(length) = u64::try_from(length) else {
        return Ok(());
    };
    if cursor.taken > end || length > end - cursor.taken {
        return Err(Stop::Number);
    }
    cursor.skip(length)
}

fn skip_headers<R: BufRead>(cursor: &mut Cursor<R>, end: u64) -> Result<(), Stop> {
    let count = cursor.varint()?;
    if count < 0 || cursor.taken > end || count as u64 > end - cursor.taken {
        return Err(Stop::Number);
    }
    for _ in 0..count {
        let key = cursor.varint()?;
        if key < 0 {
            return Err(Stop::Number);
        }
        skip_field(cursor, key, end)?;
        let value = cursor.varint()?;
        skip_field(cursor, value, end)?;
    }
    Ok(())
}

impl Batch {
    // fuzz:parser fuzz_batch
    /// Reads every record of the batch, through its codec, and holds them to `rules`.
    ///
    /// # Errors
    /// The first rule broken, as Kafka answers it.
    pub fn scan(
        &self,
        rules: &ScanRules,
        topic_partition: &dyn Display,
    ) -> Result<Scanned, RecordsRefused> {
        let codec = self.attributes().compression();
        let body = self.as_bytes().get(super::HEADER_LEN..).unwrap_or_default();
        let len = self.len();
        if codec == Compression::None {
            return self.scan_records(
                Cursor {
                    source: body,
                    taken: 0,
                },
                rules,
                topic_partition,
            );
        }
        let stream = inflate::open(codec, body, rules.max_inflated)
            .map_err(|e| RecordsRefused::inflating(e, len))?;
        let source = BufReader::with_capacity(BUFFER, stream);
        self.scan_records(Cursor { source, taken: 0 }, rules, topic_partition)
    }

    fn scan_records<R: BufRead>(
        &self,
        mut cursor: Cursor<R>,
        rules: &ScanRules,
        topic_partition: &dyn Display,
    ) -> Result<Scanned, RecordsRefused> {
        let count = i32::try_from(self.record_count()).unwrap_or(i32::MAX);
        let (first, len) = (self.first_timestamp().get(), self.len());
        let client_stamped = self.attributes().timestamp_type() == super::TimestampType::LogAppend;
        let mut judge = Judge::new(rules, topic_partition, client_stamped);
        let mut sequential = true;
        for index in 0..count {
            let record = read_record(&mut cursor, index, count).map_err(|f| refusal_of(f, len))?;
            sequential &= record.offset_delta == i64::from(index);
            let timestamp = first.saturating_add(record.timestamp_delta);
            judge.record(index, timestamp, record.key_length);
        }
        // Nothing may follow the last record the header counts.
        match cursor.byte() {
            Err(Stop::Cut | Stop::Number) => {}
            Ok(_) => {
                return Err(RecordsRefused::invalid(format!(
                    "the batch says it holds {count} records and more bytes follow them"
                )));
            }
            Err(Stop::Source(e)) => return Err(refusal_of(Fault::Source(e), len)),
        }
        // Offsets inside the batch that are not 0, 1, 2: Kafka keeps an uncompressed batch like that as it came, and
        // fails on a compressed one it would have kept as it came.
        if !sequential && self.attributes().compression() != Compression::None {
            return Err(RecordsRefused::new(
                Refusal::UnknownServerError,
                false,
                "the records of a compressed batch are not numbered 0, 1, 2, ...",
            ));
        }
        judge.finish()
    }
}

/// The refusal for a record that could not be read.
fn refusal_of(fault: Fault, batch_bytes: usize) -> RecordsRefused {
    match fault {
        Fault::Malformed(why) => RecordsRefused::invalid(why),
        Fault::Source(e) => RecordsRefused::inflating(inflate::why(&e), batch_bytes),
    }
}

#[cfg(test)]
mod tests;
