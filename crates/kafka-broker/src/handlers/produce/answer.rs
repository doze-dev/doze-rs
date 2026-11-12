//! Waiting for a queued append, and the shapes of a partition's answer.

use kafka_storage::fs::Fs;
use kafka_wire::ids::TimestampMs;
use kafka_wire::messages::produce_response::PartitionProduceResponse;
use kafka_wire::{ResponseError, StrBytes};

use super::Stage;
use crate::partitions::{Partition, Pending, Written, gone};

/// Waits for a queued append to be written (and, for `acks=-1`, flushed) and builds its answer.
pub(super) async fn finish<F: Fs>(stage: Stage<F>, durable: bool) -> PartitionProduceResponse {
    match stage {
        Stage::Done(done) => done,
        Stage::Waiting {
            answer,
            partition,
            pending,
            appended_at,
        } => wait_for(answer, &partition, pending, appended_at, durable).await,
    }
}

async fn wait_for<F: Fs>(
    mut answer: PartitionProduceResponse,
    partition: &Partition<F>,
    pending: Pending,
    appended_at: Option<TimestampMs>,
    durable: bool,
) -> PartitionProduceResponse {
    let appended = match pending.await {
        Ok(Ok(Written::Appended(appended))) => appended,
        // A retry of a batch already written is answered as the first was: where it went, and its timestamp as stored.
        Ok(Ok(Written::Duplicate { offset, timestamp })) => {
            answer.base_offset = offset.get();
            answer.log_append_time_ms = timestamp;
            answer.log_start_offset = partition.reader().start_offset().get();
            return answer;
        }
        Ok(Ok(Written::Refused(code))) => {
            let mut refused = failed(answer.index, code, None);
            refused.log_start_offset = partition.reader().start_offset().get();
            return refused;
        }
        Ok(Err(e)) => return storage_fault(answer.index, &e),
        Err(_) => return storage_fault(answer.index, &gone()),
    };
    if durable {
        // acks=all is a promise that the record is on disk: if the disk says otherwise the producer hears it.
        if let Err(e) = appended.ticket.durable().await {
            return storage_fault(answer.index, &e);
        }
    }
    answer.base_offset = appended.first_offset.get();
    answer.log_append_time_ms = appended_at.map_or(-1, TimestampMs::get);
    answer.log_start_offset = partition.reader().start_offset().get();
    answer
}

/// An answer with no error and no offsets yet.
pub(super) fn blank(index: i32) -> PartitionProduceResponse {
    let mut r = PartitionProduceResponse::default();
    r.index = index;
    r.base_offset = -1;
    r.log_append_time_ms = -1;
    r.log_start_offset = -1;
    r.error_message = None;
    r
}

/// An answer that carries an error and no offsets.
pub(super) fn failed(
    index: i32,
    code: ResponseError,
    message: Option<String>,
) -> PartitionProduceResponse {
    let mut r = blank(index);
    r.error_code = code.code();
    r.error_message = message.map(StrBytes::from_string);
    r
}

/// A write or flush that failed, as Kafka answers a full disk: the code, no offsets and no message (the cause is
/// in the broker's log, not in a client's).
pub(super) fn storage_fault(
    index: i32,
    e: &kafka_storage::StorageError,
) -> PartitionProduceResponse {
    tracing::error!(error = %e, partition = index, "a produce could not be stored");
    failed(index, ResponseError::KafkaStorageError, None)
}
