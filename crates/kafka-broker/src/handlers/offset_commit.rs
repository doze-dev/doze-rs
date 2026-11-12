//! OffsetCommit: a group's progress, stored in the metadata database.

use kafka_auth::authz::operation;
use kafka_meta::{Committed, MetaError, OffsetCommit, StoredOffset};
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::ids::{GroupId, InstanceId, MemberId, PartitionId, TopicName};
use kafka_wire::messages::offset_commit_response::{
    OffsetCommitResponsePartition, OffsetCommitResponseTopic,
};
use kafka_wire::messages::{OffsetCommitRequest, OffsetCommitResponse};

use crate::groups::{Answer, CommitRequest, Req};
use crate::{Broker, Caller, Outcome, blocking, clock};

/// Kafka's `offset.metadata.max.bytes`: the longest text a commit may carry.
const MAX_METADATA: usize = 4096;

impl<F: Fs> Broker<F> {
    /// Stores what each partition of the request says its group has reached.
    ///
    /// The caller must be allowed to READ the group (30 for every partition if not), and each topic it commits to (29 for
    /// that topic's partitions): before the topic is looked for, and before the group is asked about the committer.
    pub(crate) async fn offset_commit(
        &self,
        version: i16,
        caller: &Caller,
        req: &OffsetCommitRequest,
    ) -> Outcome {
        let guard = self.guard(caller);
        if !guard.group(operation::READ, req.group_id.as_str()) {
            return blanket(req, ResponseError::GroupAuthorizationFailed);
        }
        let barred = |name: &str| !guard.topic(operation::READ, name);
        let fence = self.fence_unless_barred(version, req, &barred).await;
        let mut resp = OffsetCommitResponse::default();
        // Each partition is judged; those that pass are stored together in one write.
        let mut to_store: Vec<OffsetCommit> = Vec::new();
        let mut slots: Vec<(usize, usize)> = Vec::new();
        let now = clock::now().get();
        for (t, topic) in req.topics.iter().enumerate() {
            let mut out = OffsetCommitResponseTopic::default();
            out.name = topic.name.clone();
            out.topic_id = topic.topic_id;
            let name = TopicName::parse(topic.name.as_str()).ok();
            let refused_topic = barred(topic.name.as_str());
            for (p, partition) in topic.partitions.iter().enumerate() {
                let mut answer = OffsetCommitResponsePartition::default();
                answer.partition_index = partition.partition_index;
                let known = name.as_ref().and_then(|n| self.meta.topic(n)).filter(|t| {
                    partition.partition_index >= 0 && partition.partition_index < t.partitions.get()
                });
                let metadata = partition
                    .committed_metadata
                    .as_ref()
                    .map_or("", |m| m.as_str());
                let refusal = if refused_topic {
                    Some(ResponseError::TopicAuthorizationFailed)
                } else if let Some(e) = fence {
                    Some(e)
                } else if known.is_none() {
                    Some(ResponseError::UnknownTopicOrPartition)
                } else if metadata.len() > MAX_METADATA {
                    Some(ResponseError::OffsetMetadataTooLarge)
                } else {
                    None
                };
                match (refusal, &name, PartitionId::new(partition.partition_index)) {
                    (Some(e), _, _) => answer.error_code = e.code(),
                    (None, Some(name), Some(id)) => {
                        to_store.push(OffsetCommit {
                            topic: name.clone(),
                            partition: id,
                            offset: StoredOffset {
                                offset: partition.committed_offset,
                                leader_epoch: partition.committed_leader_epoch,
                                metadata: metadata.to_owned(),
                                committed_ms: now,
                            },
                        });
                        slots.push((t, p));
                    }
                    (None, _, _) => {
                        answer.error_code = ResponseError::UnknownTopicOrPartition.code()
                    }
                }
                out.partitions.push(answer);
            }
            resp.topics.push(out);
        }
        if !to_store.is_empty() {
            self.store(req, &to_store, &slots, &mut resp);
        }
        Outcome::Respond(kafka_wire::api::Response::OffsetCommit(resp))
    }

    /// [`Self::commit_fence`], except that Kafka asks the group nothing when no topic is left to commit to.
    async fn fence_unless_barred(
        &self,
        version: i16,
        req: &OffsetCommitRequest,
        barred: &impl Fn(&str) -> bool,
    ) -> Option<ResponseError> {
        let none_left =
            !req.topics.is_empty() && req.topics.iter().all(|t| barred(t.name.as_str()));
        if none_left {
            None
        } else {
            self.commit_fence(version, req).await
        }
    }

    /// May this group's offsets be written by this committer? `None`: yes.
    async fn commit_fence(&self, version: i16, req: &OffsetCommitRequest) -> Option<ResponseError> {
        let Ok(group) = GroupId::parse(req.group_id.as_str()) else {
            return Some(ResponseError::InvalidGroupId);
        };
        let check = CommitRequest {
            version,
            member_id: MemberId::new(req.member_id.as_str()),
            instance: req
                .group_instance_id
                .as_ref()
                .map(|i| InstanceId::new(i.as_str())),
            generation: req.generation_id_or_member_epoch,
            transactional: false,
        };
        self.ask_group_to_commit(&group, check).await
    }

    /// What the group says to a committer: `None`, or why its offsets may not be written.
    pub(crate) async fn ask_group_to_commit(
        &self,
        group: &GroupId,
        check: CommitRequest,
    ) -> Option<ResponseError> {
        let (member, generation, version) =
            (check.member_id.clone(), check.generation, check.version);
        let fenced = match self.groups.ask(group, Req::Commit(check)).await {
            Some(Answer::Commit(error)) => error,
            // The group has gone while asking: nothing is known of the committer.
            _ => Some(ResponseError::UnknownMemberId),
        };
        if let Some(error) = fenced {
            tracing::debug!(%group, member = member.as_ref().map(MemberId::as_str), generation, version, ?error, "an offset commit was refused by its group");
        }
        fenced
    }

    /// Stores what passed and tells each partition how it went.
    fn store(
        &self,
        req: &OffsetCommitRequest,
        commits: &[OffsetCommit],
        slots: &[(usize, usize)],
        resp: &mut OffsetCommitResponse,
    ) {
        let Ok(group) = GroupId::parse(req.group_id.as_str()) else {
            return;
        };
        let result = blocking::run(|| self.meta.commit_offsets(&group, commits));
        let codes: Vec<i16> = match result {
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
                tracing::error!(group = %group, error = %e, "storing committed offsets failed");
                let code = match e {
                    MetaError::Database(_) | MetaError::Io(_) => ResponseError::NotCoordinator,
                    _ => ResponseError::UnknownServerError,
                };
                vec![code.code(); commits.len()]
            }
        };
        for ((t, p), code) in slots.iter().zip(codes) {
            if let Some(answer) = resp
                .topics
                .get_mut(*t)
                .and_then(|topic| topic.partitions.get_mut(*p))
            {
                answer.error_code = code;
            }
        }
    }
}

/// The answer when the whole request is refused: every partition of every topic, with the code.
fn blanket(req: &OffsetCommitRequest, error: ResponseError) -> Outcome {
    let mut resp = OffsetCommitResponse::default();
    for topic in &req.topics {
        let mut out = OffsetCommitResponseTopic::default();
        out.name = topic.name.clone();
        out.topic_id = topic.topic_id;
        for partition in &topic.partitions {
            let mut answer = OffsetCommitResponsePartition::default();
            answer.partition_index = partition.partition_index;
            answer.error_code = error.code();
            out.partitions.push(answer);
        }
        resp.topics.push(out);
    }
    Outcome::Respond(kafka_wire::api::Response::OffsetCommit(resp))
}
