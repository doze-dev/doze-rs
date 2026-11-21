//! A partition read for a consumer of Fetch v0 to v3, which reads message format v0 (Fetch v0 and v1) or v1 (v2 and
//! v3), not the v2 batches the log holds (fetch.md F-OLD).
//!
//! Kafka cuts what it reads of the log as a file slice: from the batch that holds the fetch offset, `partition_max_bytes`
//! bytes, the last batch possibly cut short. Up to v2 that limit is hard, and a first batch larger than it comes back cut
//! (a consumer of that age asks again with a larger limit); from v3 (KIP-74) the first batch comes back whole however
//! large. That slice is then down-converted (`kafka_wire::batch::legacy::down`), filled as the release the broker stands
//! for fills it. Nothing is read uncommitted-only: v0 to v3 have no isolation level, and an old consumer reads aborted
//! records too (it is told of no transactions).

use bytes::{Bytes, BytesMut};
use kafka_storage::fs::Fs;
use kafka_storage::{LogReader, StorageError};
use kafka_wire::ResponseError;
use kafka_wire::batch::Peek;
use kafka_wire::batch::legacy::{self, DownError, DownRules, Downconversion, Magic};
use kafka_wire::ids::Offset;
use kafka_wire::messages::fetch_request::FetchPartition;
use kafka_wire::messages::fetch_response::PartitionData;

use super::{Budget, How, take};

/// The first Fetch version that reads v2 batches as they are.
pub(crate) const BATCHES_FROM: i16 = 4;
/// The first version that may read past its limit to return one whole batch (KIP-74).
const WHOLE_FIRST_FROM: i16 = 3;
/// The first version that reads message format v1 (KIP-32).
const V1_FROM: i16 = 2;
/// The most one stored batch may inflate to as it is converted.
const MAX_INFLATED: u64 = 64 << 20;

/// Reads one partition for an old consumer into `data`; returns the bytes read (before conversion).
pub(super) fn fill<F: Fs>(
    data: &mut PartitionData,
    reader: &LogReader<F>,
    asked: &FetchPartition,
    limit: usize,
    how: How,
    budget: &mut Budget,
) -> usize {
    let read = Offset::new(asked.fetch_offset)
        .ok_or(StorageError::OutOfRange {
            requested: asked.fetch_offset,
            start: reader.start_offset().get(),
            end: reader.end_offset().get(),
        })
        .and_then(|from| slice(reader, from, limit, how.version));
    data.high_watermark = reader.end_offset().get();
    data.last_stable_offset = data.high_watermark;
    data.log_start_offset = reader.start_offset().get();
    match read {
        Ok(read) => {
            let n = take(data, read, budget);
            if let Some(records) = data.records.take() {
                data.records = convert(data, &records, asked.fetch_offset, how);
            }
            n
        }
        Err(StorageError::OutOfRange { .. }) => {
            data.error_code = ResponseError::OffsetOutOfRange.code();
            (
                data.high_watermark,
                data.last_stable_offset,
                data.log_start_offset,
            ) = (-1, -1, -1);
            data.records = Some(Bytes::new());
            0
        }
        Err(e) => {
            tracing::error!(error = %e, "a fetch could not read a partition");
            data.error_code = ResponseError::KafkaStorageError.code();
            0
        }
    }
}

/// What Kafka's slice of the log holds: from the batch that holds `from`, `limit` bytes (from v3, at least the first
/// batch whole), the last batch cut where the slice ends.
fn slice<F: Fs>(
    reader: &LogReader<F>,
    from: Offset,
    limit: usize,
    version: i16,
) -> Result<Bytes, StorageError> {
    let whole = reader.read(from, limit, None)?;
    let Ok(first) = Peek::read(&whole) else {
        return Ok(whole);
    };
    let first_len = usize::try_from(first.total_len).unwrap_or(usize::MAX);
    let target = if version >= WHOLE_FIRST_FROM {
        limit.max(first_len)
    } else {
        limit
    };
    if whole.len() >= target {
        return Ok(whole.slice(..target));
    }
    // The whole batches fall short of the slice: the rest of it is the start of the batch after them.
    let Some(next) = next_offset(&whole).and_then(Offset::new) else {
        return Ok(whole);
    };
    if next >= reader.end_offset() {
        return Ok(whole);
    }
    let more = reader.read(next, target - whole.len(), None)?;
    let wanted = more.len().min(target - whole.len());
    let mut out = BytesMut::with_capacity(whole.len() + wanted);
    out.extend_from_slice(&whole);
    out.extend_from_slice(more.get(..wanted).unwrap_or_default());
    Ok(out.freeze())
}

/// The offset after the last whole batch of `bytes`.
fn next_offset(bytes: &[u8]) -> Option<i64> {
    let mut at = 0_usize;
    let mut next = None;
    while let Some(Ok(peek)) = bytes.get(at..).map(Peek::read) {
        let len = usize::try_from(peek.total_len).ok()?;
        if at + len > bytes.len() {
            break;
        }
        next = peek.next_offset();
        at += len;
    }
    next
}

/// The read, as an old consumer is answered; a fault is the partition's error, with the records the release answers it with.
fn convert(data: &mut PartitionData, read: &Bytes, fetch_offset: i64, how: How) -> Option<Bytes> {
    let rules = DownRules {
        to: if how.version >= V1_FROM {
            Magic::V1
        } else {
            Magic::V0
        },
        fetch_offset,
        how: match how.kafka.get() {
            1 => Downconversion::Eager,
            2 => Downconversion::Chunked,
            _ => Downconversion::ChunkedAlways,
        },
        max_inflated: MAX_INFLATED,
    };
    match legacy::down(read, &rules) {
        Ok(converted) => Some(converted),
        Err(e) => {
            let code = match e {
                DownError::Zstd => ResponseError::UnsupportedCompressionType,
                other => {
                    tracing::error!(error = %other, "a fetch could not convert a partition for an old consumer");
                    ResponseError::KafkaStorageError
                }
            };
            data.error_code = code.code();
            (
                data.high_watermark,
                data.last_stable_offset,
                data.log_start_offset,
            ) = (-1, -1, -1);
            data.aborted_transactions = Some(Vec::new());
            as_the_release_refuses(data, how);
            data.records.take()
        }
    }
}

/// A partition refused before it was read, as each release answers it: from Kafka 3, with null records and an empty list
/// of aborted transactions (`FetchResponse.partitionResponse`); Kafka 1.1 and 2.8, with empty records and no list (their
/// `FetchResponse.PartitionData`), as the recorded 2.8 and 3.9 answer a zstd topic.
pub(in crate::handlers::fetch) fn as_the_release_refuses(data: &mut PartitionData, how: How) {
    if how.kafka.get() <= 2 {
        data.aborted_transactions = None;
        data.records = Some(Bytes::new());
    } else {
        data.records = None;
    }
}
