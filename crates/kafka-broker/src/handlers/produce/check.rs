//! What one partition's batch is held to before it is stored.

use bytes::Bytes;
use kafka_meta::Topic;
use kafka_wire::ResponseError;
use kafka_wire::batch::legacy::{self, Up, UpRules};
use kafka_wire::batch::{
    Batch, Compression, HEADER_LEN, ProduceRules, RecordError, RecordsRefused, Refusal, ScanRules,
    Scanned, TimestampType,
};
use kafka_wire::ids::{PartitionId, TimestampMs};
use kafka_wire::profile::KafkaMajor;

use crate::Defaults;

/// The most one batch may inflate to when its records are read: sixty-four times the Java producer's default request
/// size. Real Kafka has no such bound; a batch past it is refused with Kafka's size error.
const MAX_INFLATED: u64 = 64 << 20;
/// How far ahead of the broker's clock a CreateTime record may be by default (`message.timestamp.after.max.ms`): an hour
/// from Kafka 4.0 (KIP-937); before it, no bound (the recorded 2.8 and 3.9 take a record three days ahead).
fn after_max_ms(kafka: KafkaMajor) -> i64 {
    if kafka.get() >= 4 {
        3_600_000
    } else {
        i64::MAX
    }
}

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

/// What every partition of a request is checked against: the broker's defaults, and the release it stands for.
#[derive(Clone, Copy)]
pub(super) struct At<'a> {
    pub(super) defaults: &'a Defaults,
    pub(super) kafka: KafkaMajor,
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
    at: &At<'_>,
    version: i16,
    topic: &Topic,
    partition: PartitionId,
    records: &Bytes,
    now: TimestampMs,
) -> Result<Accepted, Refused> {
    let At { defaults, kafka } = *at;
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
        after_max_ms: settings
            .timestamp_after_max_ms
            .unwrap_or(after_max_ms(kafka)),
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

/// The batch a Produce v0 to v2 message set becomes (`batch::legacy::up`), held to the topic's rules as a v3 batch is, or
/// `None` for a set with no whole message, which appends nothing and is answered with no offset. `kafka` is the release the
/// broker stands for: Kafka 1.1 holds the set to no shape, and knows no zstd.
pub(super) fn check_old(
    at: &At<'_>,
    topic: &Topic,
    partition: PartitionId,
    records: &Bytes,
    now: TimestampMs,
) -> Result<Option<Accepted>, Refused> {
    let At { defaults, kafka } = *at;
    let settings = topic.settings();
    let max = u64::try_from(
        settings
            .max_message_bytes
            .unwrap_or(defaults.message_max_bytes),
    )
    .unwrap_or(0);
    let rules = UpRules {
        strict: kafka.get() >= 2,
        knows_zstd: kafka.get() >= 2,
        max_message_bytes: max,
        max_inflated: MAX_INFLATED,
    };
    let refused = |e: legacy::UpError| {
        tracing::debug!(topic = %topic.name, partition = %partition, why = %e, "an old message set was refused");
        Refused {
            code: code_of(e.refusal),
            before_log: true,
            message: None,
            records: Vec::new(),
        }
    };
    let (batch, source) = match legacy::up(records, &rules).map_err(refused)? {
        Up::Empty => return Ok(None),
        // One v2 batch is taken as from v3, except that zstd's floor (v7) is not asked: Kafka holds only v3 and later
        // requests to it (`ProduceRequest.validateRecords`).
        Up::Batch(batch) => return check(at, ZSTD_FROM, topic, partition, &batch, now).map(Some),
        Up::Converted { batch, source } => (batch, source),
    };
    let log_append_time = settings.timestamp_type == TimestampType::LogAppend;
    let scan = ScanRules {
        require_keys: settings.cleanup.compact,
        log_append_time,
        now,
        before_max_ms: settings.timestamp_before_max_ms.unwrap_or(i64::MAX),
        after_max_ms: settings
            .timestamp_after_max_ms
            .unwrap_or(after_max_ms(kafka)),
        max_inflated: MAX_INFLATED,
        renumbering: false,
    };
    let scanned = batch
        .scan(&scan, &format_args!("{}-{partition}", topic.name))
        .map_err(Refused::from)?;
    // Written in the topic's codec, or the producer's under `producer`, and held to the size as stored (Kafka checks the
    // batch it built, `LogValidator`'s `messageSizeMaybeChanged`).
    let target = settings.compression.unwrap_or(source);
    let batch = if target == Compression::None {
        if batch.len() as u64 > max {
            return Err(Refused::too_large());
        }
        batch
    } else {
        written_again(&batch, target, &scanned, max)?
    };
    Ok(Some(Accepted {
        batch,
        log_append_time,
    }))
}

/// The first Produce version that may carry zstd.
const ZSTD_FROM: i16 = 7;

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
