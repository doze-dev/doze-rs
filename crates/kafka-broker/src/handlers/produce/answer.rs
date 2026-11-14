//! Waiting for a queued append, and the shapes of a partition's answer.

use kafka_storage::StorageError;
use kafka_storage::fs::Fs;
use kafka_wire::ids::{TimestampMs, TopicId};
use kafka_wire::messages::produce_response::PartitionProduceResponse;
use kafka_wire::{ResponseError, StrBytes};

use super::Stage;
use crate::partitions::{Partition, Pending, Written, gone};

/// Waits for a queued append to be written (and, for `acks=-1`, flushed) and builds its answer.
/// `deleted` says whether a topic has been deleted since the request looked it up.
pub(super) async fn finish<F: Fs>(
    stage: Stage<F>,
    durable: bool,
    deleted: impl Fn(TopicId) -> bool,
) -> PartitionProduceResponse {
    match stage {
        Stage::Done(done) => done,
        Stage::Waiting {
            answer,
            topic,
            partition,
            pending,
            appended_at,
        } => {
            let index = answer.index;
            wait_for(answer, &partition, pending, appended_at, durable)
                .await
                .unwrap_or_else(|e| storage_fault(index, &e, deleted(topic)))
        }
    }
}

async fn wait_for<F: Fs>(
    mut answer: PartitionProduceResponse,
    partition: &Partition<F>,
    pending: Pending,
    appended_at: Option<TimestampMs>,
    durable: bool,
) -> Result<PartitionProduceResponse, StorageError> {
    let appended = match pending.await {
        Ok(Ok(Written::Appended(appended))) => appended,
        // A retry of a batch already written is answered as the first was: where it went, and its timestamp as stored.
        Ok(Ok(Written::Duplicate { offset, timestamp })) => {
            answer.base_offset = offset.get();
            answer.log_append_time_ms = timestamp;
            answer.log_start_offset = partition.reader().start_offset().get();
            return Ok(answer);
        }
        Ok(Ok(Written::Refused(code))) => {
            let mut refused = failed(answer.index, code, None);
            refused.log_start_offset = partition.reader().start_offset().get();
            return Ok(refused);
        }
        Ok(Err(e)) => return Err(e),
        Err(_) => return Err(gone()),
    };
    if durable {
        // acks=all is a promise that the record is on disk: if the disk says otherwise the producer hears it.
        appended.ticket.durable().await?;
    }
    answer.base_offset = appended.first_offset.get();
    answer.log_append_time_ms = appended_at.map_or(-1, TimestampMs::get);
    answer.log_start_offset = partition.reader().start_offset().get();
    Ok(answer)
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

/// A partition that could not be opened, or a write or flush that failed.
///
/// If the topic was `deleted` since the request looked it up (a Produce racing a DeleteTopics: the partition's
/// files go, or its writer closes, under the append), the partition is no longer hosted, and Kafka answers that
/// UNKNOWN_TOPIC_OR_PARTITION (`ReplicaManager.getPartitionOrException` once the metadata has dropped the topic): a
/// client's ordinary race, logged at debug. Otherwise it is the disk, answered as Kafka answers a full one: the code,
/// no offsets and no message (the cause is in the broker's log, not in a client's).
pub(super) fn storage_fault(index: i32, e: &StorageError, deleted: bool) -> PartitionProduceResponse {
    if deleted {
        tracing::debug!(error = %e, partition = index, "a produce raced the delete of its topic");
        return failed(index, ResponseError::UnknownTopicOrPartition, None);
    }
    tracing::error!(error = %e, partition = index, "a produce could not be stored");
    failed(index, ResponseError::KafkaStorageError, None)
}
