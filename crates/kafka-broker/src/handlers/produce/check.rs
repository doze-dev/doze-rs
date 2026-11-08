//! What one partition's batch is held to before it is stored.

use bytes::Bytes;
use kafka_meta::Topic;
use kafka_wire::ResponseError;
use kafka_wire::batch::{Batch, ProduceRules, RecordError, Refusal, ScanRules};
use kafka_wire::ids::{PartitionId, TimestampMs};

use crate::Config;

/// The most one batch may inflate to when its records are read: sixty-four times the Java producer's default request
/// size. Real Kafka has no such bound; a batch past it is refused with Kafka's size error.
const MAX_INFLATED: u64 = 64 << 20;
/// How far ahead of the broker's clock a CreateTime record may be (`message.timestamp.after.max.ms`).
const AFTER_MAX_MS: i64 = 3_600_000;

/// Why a batch is not stored, and how it is answered.
pub(super) struct Refused {
    pub(super) code: ResponseError,
    /// Kafka refuses it before it has asked the partition anything, so its answer has no log start offset.
    pub(super) before_log: bool,
    pub(super) message: Option<String>,
    pub(super) records: Vec<RecordError>,
}

/// A batch that passed, and whether its topic stamps records with the broker's clock.
pub(super) struct Accepted {
    pub(super) batch: Batch,
    pub(super) log_append_time: bool,
}

/// The Kafka error code for each way of refusing.
pub(super) fn code_of(refusal: Refusal) -> ResponseError {
    match refusal {
        Refusal::CorruptMessage => ResponseError::CorruptMessage,
        Refusal::InvalidRecord => ResponseError::InvalidRecord,
        Refusal::MessageTooLarge => ResponseError::MessageTooLarge,
        Refusal::UnsupportedCompression => ResponseError::UnsupportedCompressionType,
        Refusal::InvalidTimestamp => ResponseError::InvalidTimestamp,
        _ => ResponseError::UnknownServerError,
    }
}

/// Holds the one batch of `records` to Kafka's rules and the topic's, in Kafka's order: the request's shape and the
/// batch's header and size, then every record. On success the batch's newest timestamp is the records' own.
pub(super) fn check(
    config: &Config,
    version: i16,
    topic: &Topic,
    partition: PartitionId,
    records: &Bytes,
    now: TimestampMs,
) -> Result<Accepted, Refused> {
    let own = |key: &str| topic.config.get(key).and_then(|v| v.parse::<i64>().ok());
    let max_batch = own("max.message.bytes").unwrap_or(config.defaults.message_max_bytes);
    let rules = ProduceRules {
        version,
        max_batch_bytes: u64::try_from(max_batch).unwrap_or(0),
    };
    let batch = Batch::from_produce(records, rules).map_err(|e| Refused {
        code: code_of(e.refusal()),
        before_log: e.before_log(),
        message: e.kafka_message(),
        records: Vec::new(),
    })?;
    let log_append_time = topic
        .config
        .get("message.timestamp.type")
        .is_some_and(|v| v == "LogAppendTime");
    let scan = ScanRules {
        require_keys: topic
            .config
            .get("cleanup.policy")
            .is_some_and(|v| v.split(',').any(|p| p.trim() == "compact")),
        log_append_time,
        now,
        before_max_ms: own("message.timestamp.before.max.ms").unwrap_or(i64::MAX),
        after_max_ms: own("message.timestamp.after.max.ms").unwrap_or(AFTER_MAX_MS),
        max_inflated: MAX_INFLATED,
    };
    let scanned = batch
        .scan(&scan, &format_args!("{}-{partition}", topic.name))
        .map_err(|r| {
            tracing::debug!(topic = %topic.name, partition = %partition, why = %r.reason, "a batch was refused");
            Refused {
                code: code_of(r.refusal),
                before_log: r.before_log,
                message: r.message,
                records: r.records,
            }
        })?;
    // The header's newest timestamp is the broker's to say, not the producer's: the latest of the records'.
    let batch = if log_append_time {
        batch
    } else {
        batch.with_max_timestamp(scanned.max_timestamp)
    };
    Ok(Accepted {
        batch,
        log_append_time,
    })
}
