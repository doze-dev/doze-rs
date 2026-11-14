//! What one partition's batch is held to before it is stored.

use bytes::Bytes;
use kafka_meta::Topic;
use kafka_wire::ResponseError;
use kafka_wire::batch::{
    Batch, Compression, HEADER_LEN, ProduceRules, RecordError, RecordsRefused, Refusal, ScanRules,
    Scanned, TimestampType,
};
use kafka_wire::ids::{PartitionId, TimestampMs};

use crate::Defaults;

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

impl Refused {
    fn too_large() -> Self {
        Self {
            code: ResponseError::MessageTooLarge,
            before_log: true,
            message: None,
            records: Vec::new(),
        }
    }
}

impl From<RecordsRefused> for Refused {
    fn from(r: RecordsRefused) -> Self {
        Self {
            code: code_of(r.refusal),
            before_log: r.before_log,
            message: r.message,
            records: r.records,
        }
    }
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
/// batch's header and size, then every record, then the size it is stored at. On success the batch's newest
/// timestamp is the records' own, and its records are in the codec the topic stores.
pub(super) fn check(
    defaults: &Defaults,
    version: i16,
    topic: &Topic,
    partition: PartitionId,
    records: &Bytes,
    now: TimestampMs,
) -> Result<Accepted, Refused> {
    let settings = topic.settings();
    let max_batch = settings
        .max_message_bytes
        .unwrap_or(defaults.message_max_bytes);
    let max = u64::try_from(max_batch).unwrap_or(0);
    let rules = ProduceRules {
        version,
        max_batch_bytes: max,
    };
    let batch = Batch::from_produce(records, rules).map_err(|e| Refused {
        code: code_of(e.refusal()),
        before_log: e.before_log(),
        message: e.kafka_message(),
        records: Vec::new(),
    })?;
    let log_append_time = settings.timestamp_type == TimestampType::LogAppend;
    // The codec the topic stores: the producer's unless `compression.type` names another.
    let produced = batch.attributes().compression();
    let target = settings.compression.unwrap_or(produced);
    let scan = ScanRules {
        require_keys: settings.cleanup.compact,
        log_append_time,
        now,
        before_max_ms: settings.timestamp_before_max_ms.unwrap_or(i64::MAX),
        after_max_ms: settings.timestamp_after_max_ms.unwrap_or(AFTER_MAX_MS),
        max_inflated: MAX_INFLATED,
        renumbering: target != produced,
    };
    let scanned = batch
        .scan(&scan, &format_args!("{}-{partition}", topic.name))
        .map_err(|r| {
            tracing::debug!(topic = %topic.name, partition = %partition, why = %r.reason, "a batch was refused");
            Refused::from(r)
        })?;
    let batch = if target == produced {
        // The header's newest timestamp is the broker's to say, not the producer's: the latest of the records'.
        if log_append_time {
            batch
        } else {
            batch.with_max_timestamp(scanned.max_timestamp)
        }
    } else {
        written_again(&batch, target, &scanned, max)?
    };
    Ok(Accepted {
        batch,
        log_append_time,
    })
}

/// The batch in the codec the topic stores, held to the size limit as it is stored there: a batch can be within
/// the limit as it arrived and over it as the topic keeps it.
fn written_again(
    batch: &Batch,
    target: Compression,
    scanned: &Scanned,
    max: u64,
) -> Result<Batch, Refused> {
    // Uncompressed, the size is known without writing it: refused before the records are held.
    if target == Compression::None && HEADER_LEN as u64 + scanned.inflated_bytes > max {
        return Err(Refused::too_large());
    }
    let written = batch.recode(target, MAX_INFLATED, scanned.max_timestamp)?;
    if written.len() as u64 > max {
        return Err(Refused::too_large());
    }
    Ok(written)
}
