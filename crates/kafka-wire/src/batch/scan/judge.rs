//! The rules about what is inside records: keys on a compacted topic, and timestamps against the clock.

use std::fmt::Display;

use super::super::Refusal;
use super::{RecordError, RecordsRefused, ScanRules};
use crate::ids::TimestampMs;

/// A record with no timestamp (Kafka's `NO_TIMESTAMP`): it is not held to the clock.
const NO_TIMESTAMP: i64 = -1;

/// Collects the records that break a rule about their own contents.
pub(super) struct Judge<'a> {
    rules: &'a ScanRules,
    topic_partition: &'a dyn Display,
    client_stamped: bool,
    max_timestamp: i64,
    keyless: Vec<RecordError>,
    bad_time: Vec<RecordError>,
}

impl<'a> Judge<'a> {
    pub(super) fn new(
        rules: &'a ScanRules,
        topic_partition: &'a dyn Display,
        client_stamped: bool,
    ) -> Self {
        Self {
            rules,
            topic_partition,
            client_stamped,
            max_timestamp: NO_TIMESTAMP,
            keyless: Vec::new(),
            bad_time: Vec::new(),
        }
    }

    /// Judges one record, given its key and value lengths (negative: none).
    pub(super) fn record(
        &mut self,
        index: i32,
        timestamp: i64,
        (key_length, value_length): (i64, i64),
    ) {
        self.max_timestamp = self.max_timestamp.max(timestamp);
        let rules = self.rules;
        if rules.require_keys && key_length < 0 {
            self.keyless.push(RecordError {
                index,
                message: format!(
                    "Compacted topic cannot accept message without key in topic partition {}",
                    self.topic_partition
                ),
            });
        } else if !rules.log_append_time && timestamp != NO_TIMESTAMP {
            let (now, before, after) = (rules.now.get(), rules.before_max_ms, rules.after_max_ms);
            if timestamp.wrapping_sub(now) > after || now.wrapping_sub(timestamp) > before {
                // The sum wraps, as Kafka's does, and is what its message prints.
                self.bad_time.push(RecordError {
                    index,
                    message: format!(
                        "Timestamp {timestamp} of message with offset {index} is out of range. The timestamp should be within [{}, {}]",
                        now.wrapping_sub(before),
                        now.wrapping_add(after)
                    ),
                });
            }
        } else if self.client_stamped {
            // The batch says its timestamps are the broker's, and a producer is not the broker.
            self.bad_time.push(RecordError {
                index,
                message: format!(
                    "Invalid timestamp type in message DefaultRecord(offset={index}, timestamp={timestamp}, key={} bytes, value={} bytes). Producer should not set timestamp type to LogAppendTime.",
                    key_length.max(0),
                    value_length.max(0)
                ),
            });
        }
    }

    /// The newest timestamp, or why the records are refused.
    pub(super) fn finish(mut self) -> Result<TimestampMs, RecordsRefused> {
        if !self.bad_time.is_empty() {
            let mut all = std::mem::take(&mut self.keyless);
            all.append(&mut self.bad_time);
            all.sort_by_key(|r| r.index);
            return Err(RecordsRefused {
                refusal: Refusal::InvalidTimestamp,
                before_log: false,
                message: Some(
                    "One or more records have been rejected due to invalid timestamp".to_owned(),
                ),
                records: all,
                reason: "records with timestamps out of range".to_owned(),
            });
        }
        if !self.keyless.is_empty() {
            let shown: Vec<_> = self
                .keyless
                .iter()
                .take(3)
                .map(|k| {
                    format!(
                        "RecordError(batchIndex={}, message='{}')",
                        k.index, k.message
                    )
                })
                .collect();
            let message = format!(
                "One or more records have been rejected due to {} record errors in total, and only showing the first three errors at most: [{}]",
                self.keyless.len(),
                shown.join(", ")
            );
            return Err(RecordsRefused {
                refusal: Refusal::InvalidRecord,
                before_log: false,
                message: Some(message),
                records: self.keyless,
                reason: "records without keys on a compacted topic".to_owned(),
            });
        }
        Ok(TimestampMs::from_wire(self.max_timestamp))
    }
}
