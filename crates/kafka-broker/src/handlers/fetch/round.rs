//! One pass over the partitions of a Fetch.

use bytes::Bytes;
use kafka_storage::StorageError;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::ids::Offset;
use kafka_wire::messages::fetch_response::{FetchableTopicResponse, PartitionData};
use kafka_wire::messages::{FetchRequest, FetchResponse};

use super::{Place, Wanted};

/// The size of a read when neither the partition nor the request names one.
const DEFAULT_MAX_BYTES: i64 = 1 << 20;
/// A read is never asked for more than this, whatever the client says: a client's limit is not a promise that
/// there is that much to read, and a read allocates what it asks for.
const MOST_AT_ONCE: i64 = 64 << 20;

/// What a pass found.
pub(super) struct Round {
    topics: Vec<FetchableTopicResponse>,
    pub(super) bytes: usize,
    pub(super) failed: bool,
}

impl Round {
    pub(super) fn into_response(self) -> FetchResponse {
        let mut resp = FetchResponse::default();
        resp.responses = self.topics;
        resp
    }
}

/// The request-wide limit, shared out as partitions are read: each may read up to what the request has left. It is
/// soft in one way: the first batch of the first partition with data comes back even if it alone is larger, so a
/// large batch can always be fetched.
pub(super) struct Budget {
    remaining: i64,
    gave_data: bool,
}

/// The most one Fetch response holds whatever the client asks for (Kafka's `fetch.max.bytes`), so that a request for
/// "everything" cannot make the broker build a response the size of the log.
const MOST_PER_REQUEST: i64 = 50 << 20;

impl Budget {
    pub(super) fn new(max_bytes: i32) -> Self {
        Self {
            remaining: if max_bytes > 0 {
                i64::from(max_bytes).min(MOST_PER_REQUEST)
            } else {
                MOST_PER_REQUEST
            },
            gave_data: false,
        }
    }
}

pub(super) fn read<F: Fs>(
    wanted: &[Wanted<'_, F>],
    req: &FetchRequest,
    mut budget: Budget,
) -> Round {
    let mut round = Round {
        topics: Vec::new(),
        bytes: 0,
        failed: false,
    };
    for w in wanted {
        let mut topic = FetchableTopicResponse::default();
        topic.topic = w.request.topic.clone();
        topic.topic_id = w.request.topic_id;
        for (asked, place) in w.request.partitions.iter().zip(&w.places) {
            let mut data = PartitionData::default();
            data.partition_index = asked.partition;
            data.error_code = 0;
            match place {
                Place::Error(code) => {
                    data.error_code = code.code();
                    round.failed = true;
                }
                Place::Open(partition) => {
                    let limit =
                        limit_for(asked.partition_max_bytes, req.max_bytes, budget.remaining);
                    round.bytes += fill(
                        &mut data,
                        partition.reader(),
                        asked.fetch_offset,
                        limit,
                        &mut budget,
                    );
                    round.failed |= data.error_code != 0;
                }
            }
            if req.isolation_level == 1 && data.error_code == 0 {
                data.aborted_transactions = Some(Vec::new());
            } else {
                data.aborted_transactions = None;
            }
            topic.partitions.push(data);
        }
        round.topics.push(topic);
    }
    round
}

/// How much one partition may read: its own limit, else the request's, else a default; no more than the request has
/// left, and at least one byte (a read still reports the offsets and errors; the data is dropped if nothing fits).
fn limit_for(partition_max: i32, request_max: i32, remaining: i64) -> usize {
    let own = [
        i64::from(partition_max),
        i64::from(request_max),
        DEFAULT_MAX_BYTES,
    ]
    .into_iter()
    .find(|n| *n > 0)
    .unwrap_or(DEFAULT_MAX_BYTES);
    usize::try_from(own.min(remaining).clamp(1, MOST_AT_ONCE)).unwrap_or(1)
}

/// Reads one partition into `data`; returns the record bytes read.
fn fill<F: Fs>(
    data: &mut PartitionData,
    reader: &kafka_storage::LogReader<F>,
    from: i64,
    limit: usize,
    budget: &mut Budget,
) -> usize {
    let read = Offset::new(from)
        .ok_or(StorageError::OutOfRange {
            requested: from,
            start: reader.start_offset().get(),
            end: reader.end_offset().get(),
        })
        .and_then(|from| reader.read(from, limit, None));
    // The end is read after the records: what was read is at or below it.
    data.high_watermark = reader.end_offset().get();
    data.last_stable_offset = data.high_watermark;
    data.log_start_offset = reader.start_offset().get();
    match read {
        Ok(records) => take(data, records, budget),
        Err(StorageError::OutOfRange { .. }) => {
            // Kafka answers it with no offsets and empty, not null, records.
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

fn take(data: &mut PartitionData, records: Bytes, budget: &mut Budget) -> usize {
    let size = i64::try_from(records.len()).unwrap_or(i64::MAX);
    if budget.gave_data && size > budget.remaining {
        // The first batch here does not fit in what the request has left, and an earlier partition already took
        // the one oversize allowance.
        data.records = Some(Bytes::new());
        return 0;
    }
    budget.remaining = budget.remaining.saturating_sub(size);
    budget.gave_data |= size > 0;
    let len = records.len();
    data.records = Some(records);
    len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_is_held_to_what_it_asks_and_to_the_most_a_response_may_hold() {
        assert_eq!(Budget::new(1000).remaining, 1000);
        assert_eq!(Budget::new(i32::MAX).remaining, MOST_PER_REQUEST);
        assert_eq!(Budget::new(0).remaining, MOST_PER_REQUEST, "no limit asked");
        assert_eq!(Budget::new(-1).remaining, MOST_PER_REQUEST);
    }

    #[test]
    fn a_partition_reads_up_to_its_own_limit_else_the_requests_else_a_default_and_never_more_than_is_left()
     {
        assert_eq!(limit_for(500, 900, 10_000), 500);
        assert_eq!(limit_for(0, 900, 10_000), 900);
        assert_eq!(limit_for(0, 0, 10_000_000), 1 << 20);
        assert_eq!(limit_for(500, 900, 200), 200);
        assert_eq!(
            limit_for(500, 900, 0),
            1,
            "nothing left still reads a byte, for the offsets and the errors"
        );
        assert_eq!(
            limit_for(i32::MAX, 0, i64::MAX),
            64 << 20,
            "a client's limit is not a promise of that much to read"
        );
    }
}
