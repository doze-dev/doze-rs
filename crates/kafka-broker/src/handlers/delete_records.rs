//! DeleteRecords: moves a partition's start up, so that what is below it is no longer served.

use std::collections::BTreeMap;

use kafka_auth::authz::operation;
use kafka_meta::Topic;
use kafka_storage::StorageError;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::api::Response;
use kafka_wire::ids::{Offset, PartitionId, TopicName};
use kafka_wire::messages::delete_records_response::{
    DeleteRecordsPartitionResult, DeleteRecordsTopicResult,
};
use kafka_wire::messages::{DeleteRecordsRequest, DeleteRecordsResponse};

use crate::partitions::Trim;
use crate::{Broker, Caller, Outcome};

/// What Kafka answers a partition that failed: no low watermark.
const NONE: i64 = -1;
/// The offset that means the high watermark.
const HIGH_WATERMARK: i64 = -1;

impl<F: Fs> Broker<F> {
    /// Each partition asked about is judged in Kafka's order (source: `KafkaApis.handleDeleteRecordsRequest`,
    /// `Partition.deleteRecordsOnLeader`, `UnifiedLog.maybeIncrementLogStartOffset`): that it exists, that its policy lets
    /// records be deleted, that the offset is -1 (the high watermark) or at least 0, and that it is not past the high
    /// watermark; an offset at or below the start changes nothing. The answer is the low watermark now in force. A
    /// topic that names no partition has no entry in the answer, and a partition named twice is answered once, for the
    /// last offset given.
    ///
    /// The caller must be allowed to DELETE each topic; a topic it may not is answered 29 on each of its partitions,
    /// before it is looked for, so one that is not there is hidden (`KafkaApis.handleDeleteRecordsRequest`).
    pub(crate) async fn delete_records(
        &self,
        caller: &Caller,
        req: &DeleteRecordsRequest,
    ) -> Outcome {
        let mut resp = DeleteRecordsResponse::default();
        for t in &req.topics {
            let allowed = self.topic_allows(caller, &t.name.0, operation::DELETE);
            let topic = TopicName::parse(&t.name.0)
                .ok()
                .and_then(|n| self.meta.topic(&n));
            // (A partition named twice: the last wins, as in Kafka, which collects them in a map.)
            let mut asked: BTreeMap<i32, i64> = BTreeMap::new();
            for p in &t.partitions {
                asked.insert(p.partition_index, p.offset);
            }
            if asked.is_empty() {
                continue;
            }
            let mut out = DeleteRecordsTopicResult::default();
            out.name = t.name.clone();
            for (index, offset) in asked {
                let (code, low) = if allowed {
                    self.delete_in(topic.as_deref(), index, offset).await
                } else {
                    (Some(E::TopicAuthorizationFailed), NONE)
                };
                let mut answer = DeleteRecordsPartitionResult::default();
                answer.partition_index = index;
                answer.low_watermark = low;
                answer.error_code = code.map_or(0, |e| e.code());
                out.partitions.push(answer);
            }
            resp.topics.push(out);
        }
        Outcome::Respond(Response::DeleteRecords(resp))
    }

    /// One partition: the error, if any, and the low watermark.
    async fn delete_in(&self, topic: Option<&Topic>, index: i32, offset: i64) -> (Option<E>, i64) {
        let Some((topic, id)) = topic.zip(PartitionId::new(index)) else {
            return (Some(E::UnknownTopicOrPartition), NONE);
        };
        if id.get() >= topic.partitions.get() {
            return (Some(E::UnknownTopicOrPartition), NONE);
        }
        // A topic that only compacts keeps every key's latest record: deleting from it is refused.
        if crate::topic_config::compacts_only(&topic.config) {
            return (Some(E::PolicyViolation), NONE);
        }
        if offset < HIGH_WATERMARK {
            return (Some(E::OffsetOutOfRange), NONE);
        }
        let partition = match self.partitions.get(topic, id).await {
            Ok(partition) => partition,
            Err(e) => {
                tracing::error!(error = %e, topic = %topic.name, partition = index, "DeleteRecords could not open a partition");
                return (Some(E::KafkaStorageError), NONE);
            }
        };
        let to = (offset != HIGH_WATERMARK)
            .then(|| Offset::new(offset))
            .flatten();
        match partition.trim(Trim::Before(to)).await {
            Ok(start) => (None, start.get()),
            Err(StorageError::OutOfRange { .. }) => (Some(E::OffsetOutOfRange), NONE),
            Err(e) => {
                tracing::error!(error = %e, topic = %topic.name, partition = index, "DeleteRecords failed");
                (Some(E::KafkaStorageError), NONE)
            }
        }
    }
}
