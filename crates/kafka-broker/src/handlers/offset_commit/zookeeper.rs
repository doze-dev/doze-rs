//! OffsetCommit v0: the offsets Kafka 1.1 and 2.8 keep in ZooKeeper (groups.md G11).

use kafka_auth::authz::Operation;
use kafka_meta::Committed;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::ids::{GroupId, PartitionId, TopicName};
use kafka_wire::messages::offset_commit_response::{
    OffsetCommitResponsePartition, OffsetCommitResponseTopic,
};
use kafka_wire::messages::{OffsetCommitRequest, OffsetCommitResponse};

use super::{MAX_METADATA, blanket};
use crate::{Broker, Caller, Outcome, blocking};

impl<F: Fs> Broker<F> {
    /// OffsetCommit v0, which Kafka 1.1 and 2.8 store in ZooKeeper (`KafkaApis.handleOffsetCommitRequest`): the group is not
    /// asked about the committer (v0 names neither member nor generation), only the offset is kept, apart from what later
    /// versions commit, and only OffsetFetch v0 reads it back (`MetaStore::commit_zk_offsets`). The group's READ is asked
    /// (30 everywhere), then each topic's (29) and whether it exists (3), then the metadata's size (12). Kafka 3 runs without
    /// ZooKeeper: a request with anything left to store is UNSUPPORTED_VERSION on every partition (its `requireZkOrThrow`
    /// fails the whole request), one with nothing left is answered with what refused it.
    pub(super) fn offset_commit_v0(&self, caller: &Caller, req: &OffsetCommitRequest) -> Outcome {
        let guard = self.guard(caller);
        if !guard.group(Operation::Read, req.group_id.as_str()) {
            return blanket(req, ResponseError::GroupAuthorizationFailed);
        }
        let Ok(group) = GroupId::parse(req.group_id.as_str()) else {
            return blanket(req, ResponseError::InvalidGroupId);
        };
        let mut resp = OffsetCommitResponse::default();
        let (mut to_store, mut slots) = (Vec::new(), Vec::new());
        for (t, topic) in req.topics.iter().enumerate() {
            let mut out = OffsetCommitResponseTopic::default();
            out.name = topic.name.clone();
            let barred = !guard.topic(Operation::Read, topic.name.as_str());
            let name = TopicName::parse(topic.name.as_str()).ok();
            for (p, partition) in topic.partitions.iter().enumerate() {
                let mut answer = OffsetCommitResponsePartition::default();
                answer.partition_index = partition.partition_index;
                let known = name
                    .as_ref()
                    .and_then(|n| self.meta.topic(n))
                    .filter(|t| partition.partition_index < t.partitions.get());
                let metadata = partition.committed_metadata.as_ref().map_or(0, |m| m.len());
                let code = match (barred, known, PartitionId::new(partition.partition_index)) {
                    (true, ..) => Some(ResponseError::TopicAuthorizationFailed),
                    (false, Some(_), Some(_)) if metadata > MAX_METADATA => {
                        Some(ResponseError::OffsetMetadataTooLarge)
                    }
                    (false, Some(topic), Some(id)) => {
                        to_store.push((topic.name.clone(), id, partition.committed_offset));
                        slots.push((t, p));
                        None
                    }
                    _ => Some(ResponseError::UnknownTopicOrPartition),
                };
                answer.error_code = code.map_or(0, |c| c.code());
                out.partitions.push(answer);
            }
            resp.topics.push(out);
        }
        if to_store.is_empty() {
            return Outcome::Respond(kafka_wire::api::Response::OffsetCommit(resp));
        }
        if self.config.kafka_major.get() >= 3 {
            return blanket(req, ResponseError::UnsupportedVersion);
        }
        let codes = self.store_in_zookeeper(&group, &to_store);
        for ((t, p), code) in slots.iter().zip(codes) {
            if let Some(answer) = resp
                .topics
                .get_mut(*t)
                .and_then(|topic| topic.partitions.get_mut(*p))
            {
                answer.error_code = code;
            }
        }
        Outcome::Respond(kafka_wire::api::Response::OffsetCommit(resp))
    }

    /// What became of each v0 commit, as its code.
    fn store_in_zookeeper(
        &self,
        group: &GroupId,
        to_store: &[(TopicName, PartitionId, i64)],
    ) -> Vec<i16> {
        match blocking::run(|| self.meta.commit_zk_offsets(group, to_store)) {
            Ok(outcomes) => outcomes
                .iter()
                .map(|o| match o {
                    Committed::Stored => 0,
                    Committed::UnknownTopicOrPartition => {
                        ResponseError::UnknownTopicOrPartition.code()
                    }
                })
                .collect(),
            Err(e) => {
                tracing::error!(group = %group, error = %e, "storing v0 offsets failed");
                vec![ResponseError::UnknownServerError.code(); to_store.len()]
            }
        }
    }
}
