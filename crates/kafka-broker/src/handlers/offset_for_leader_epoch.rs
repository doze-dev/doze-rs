//! OffsetForLeaderEpoch (API 23): where each leader epoch of a partition ends, so that a consumer (KIP-320) or a follower
//! can tell whether the log it read from was truncated under it.
//!
//! This broker is the only replica of every partition and has led each one under epoch 0 from its first offset on
//! (Metadata says so), so a partition's leader-epoch cache holds one entry, epoch 0 from the log start offset. Kafka's
//! cache after a topic is made on a single node, and after its start has been moved (`LeaderEpochFileCache.truncateFromStart`
//! moves the first entry up to the new start), is the same. The answer is worked out from that cache by Kafka's own rule,
//! [`end_offset_for`], so it is what Kafka gives for every epoch a client can ask about.

use kafka_auth::authz::Operation;
use kafka_meta::Topic;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::ids::{PartitionId, TopicName};
use kafka_wire::messages::offset_for_leader_epoch_request::{
    OffsetForLeaderPartition, OffsetForLeaderTopic,
};
use kafka_wire::messages::offset_for_leader_epoch_response::{
    EpochEndOffset, OffsetForLeaderTopicResult,
};
use kafka_wire::messages::{OffsetForLeaderEpochRequest, OffsetForLeaderEpochResponse};

use crate::{Broker, Caller};

/// `UNDEFINED_EPOCH`: no epoch.
const UNDEFINED_EPOCH: i32 = -1;
/// `UNDEFINED_EPOCH_OFFSET`: no offset.
const UNDEFINED_EPOCH_OFFSET: i64 = -1;
/// `RecordBatch.NO_PARTITION_LEADER_EPOCH`: a request that does not say which epoch it thinks the leader is at.
const NO_CURRENT_EPOCH: i32 = -1;
/// The one leader epoch every partition here has had.
const EPOCH: i32 = 0;

/// The end of the epoch `requested`, from a partition's leader-epoch cache (`epochs`: each epoch and the offset it began
/// at, ascending) and its log end offset: `LeaderEpochFileCache.endOffsetFor`. The epoch it answers with is the one that
/// ends there: the epoch asked about if the cache has it, else the newest one before it.
///
/// - No epoch asked (-1), or one after every epoch in the cache: no epoch and no offset (-1, -1).
/// - The newest epoch: its end is the log end offset.
/// - Another epoch the cache has, or one between two it has: the start of the next epoch it has.
/// - An epoch before the first it has: the asked epoch, ending where the first one starts (the log before it is gone).
pub(crate) fn end_offset_for(epochs: &[(i32, i64)], requested: i32, log_end: i64) -> (i32, i64) {
    let undefined = (UNDEFINED_EPOCH, UNDEFINED_EPOCH_OFFSET);
    if requested == UNDEFINED_EPOCH {
        return undefined;
    }
    if epochs
        .last()
        .is_some_and(|&(latest, _)| latest == requested)
    {
        return (requested, log_end);
    }
    let Some(&(_, next_start)) = epochs.iter().find(|&&(e, _)| e > requested) else {
        return undefined;
    };
    match epochs.iter().rev().find(|&&(e, _)| e <= requested) {
        Some(&(floor, _)) => (floor, next_start),
        None => (requested, next_start),
    }
}

impl<F: Fs> Broker<F> {
    /// Answers each partition asked about. A caller with CLUSTER_ACTION on the cluster (a follower) may ask about any topic;
    /// any other needs DESCRIBE on each topic (KIP-320), and the topics it may not describe are answered 29 on every
    /// partition, after the others (`KafkaApis.handleOffsetForLeaderEpochRequest`). `replica_id` changes nothing, as in
    /// Kafka.
    pub(crate) async fn offset_for_leader_epoch(
        &self,
        caller: &Caller,
        req: &OffsetForLeaderEpochRequest,
    ) -> OffsetForLeaderEpochResponse {
        let cluster = self.cluster_allows(caller, Operation::ClusterAction);
        let mut resp = OffsetForLeaderEpochResponse::default();
        let mut refused = Vec::new();
        for t in &req.topics {
            if !cluster && !self.topic_allows(caller, &t.topic.0, Operation::Describe) {
                refused.push(answer_all(t, ResponseError::TopicAuthorizationFailed));
                continue;
            }
            let topic = TopicName::parse(&t.topic.0)
                .ok()
                .and_then(|n| self.meta.topic(&n));
            let mut out = OffsetForLeaderTopicResult::default();
            out.topic = t.topic.clone();
            for p in &t.partitions {
                out.partitions
                    .push(self.epoch_end(topic.as_deref(), p).await);
            }
            resp.topics.push(out);
        }
        resp.topics.extend(refused);
        resp
    }

    /// One partition: `ReplicaManager.lastOffsetForLeaderEpoch` and `Partition.lastOffsetForLeaderEpoch`. A partition the
    /// broker does not have is UNKNOWN_TOPIC_OR_PARTITION; then the epoch the client believes the leader is at is held to
    /// the leader's (`Partition.checkCurrentLeaderEpoch`): an older one is FENCED_LEADER_EPOCH, a newer one
    /// UNKNOWN_LEADER_EPOCH, none (-1) is not checked.
    async fn epoch_end(
        &self,
        topic: Option<&Topic>,
        p: &OffsetForLeaderPartition,
    ) -> EpochEndOffset {
        let mut r = EpochEndOffset::default();
        r.partition = p.partition;
        let refuse = |mut r: EpochEndOffset, code: ResponseError| {
            r.error_code = code.code();
            r
        };
        let Some((topic, id)) = topic.zip(PartitionId::new(p.partition)) else {
            return refuse(r, ResponseError::UnknownTopicOrPartition);
        };
        if id.get() >= topic.partitions.get() {
            return refuse(r, ResponseError::UnknownTopicOrPartition);
        }
        match p.current_leader_epoch {
            NO_CURRENT_EPOCH | EPOCH => {}
            e if e < EPOCH => return refuse(r, ResponseError::FencedLeaderEpoch),
            _ => return refuse(r, ResponseError::UnknownLeaderEpoch),
        }
        let (start, end) = match self.partitions.get_unless_empty(topic, id).await {
            Ok(Some(partition)) => {
                let reader = partition.reader();
                (reader.start_offset().get(), reader.end_offset().get())
            }
            // Nothing was ever written: the log starts and ends at 0.
            Ok(None) => (0, 0),
            Err(e) => {
                tracing::error!(error = %e, topic = %topic.name, partition = p.partition, "an epoch's end could not be read");
                return refuse(r, ResponseError::KafkaStorageError);
            }
        };
        (r.leader_epoch, r.end_offset) = end_offset_for(&[(EPOCH, start)], p.leader_epoch, end);
        r
    }
}

/// Every partition of `t` answered `code`, with no epoch and no offset.
fn answer_all(t: &OffsetForLeaderTopic, code: ResponseError) -> OffsetForLeaderTopicResult {
    let mut out = OffsetForLeaderTopicResult::default();
    out.topic = t.topic.clone();
    for p in &t.partitions {
        let mut r = EpochEndOffset::default();
        r.partition = p.partition;
        r.error_code = code.code();
        out.partitions.push(r);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_end_of_an_epoch_is_kafkas_on_the_cases_of_its_own_tests() {
        // Epochs 2, 3 and 5, begun at offsets 6, 8 and 11, as the cases of `LeaderEpochFileCacheTest` lay them out.
        let cache = [(2, 6), (3, 8), (5, 11)];
        let end = 14;
        assert_eq!(
            end_offset_for(&cache, 5, end),
            (5, 14),
            "the newest epoch ends at the log end"
        );
        assert_eq!(
            end_offset_for(&cache, 3, end),
            (3, 11),
            "an epoch ends where the next begins"
        );
        assert_eq!(
            end_offset_for(&cache, 4, end),
            (3, 11),
            "an epoch between two: the one before, to the next's start"
        );
        assert_eq!(
            end_offset_for(&cache, 1, end),
            (1, 6),
            "before the first: asked epoch, to the first's start"
        );
        assert_eq!(
            end_offset_for(&cache, 6, end),
            (-1, -1),
            "after the newest: unknown"
        );
        assert_eq!(end_offset_for(&cache, -1, end), (-1, -1), "none asked");
        assert_eq!(
            end_offset_for(&[], 0, 0),
            (-1, -1),
            "an empty cache knows no epoch"
        );
    }

    #[test]
    fn a_single_node_partition_answers_epoch_0_with_its_end() {
        let cache = [(EPOCH, 5)];
        assert_eq!(end_offset_for(&cache, 0, 42), (0, 42));
        assert_eq!(end_offset_for(&cache, 1, 42), (-1, -1));
        assert_eq!(
            end_offset_for(&cache, -2, 42),
            (-2, 5),
            "an epoch below every epoch ends at the log start"
        );
    }
}
