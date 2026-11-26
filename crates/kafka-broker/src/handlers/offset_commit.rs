//! OffsetCommit: a group's progress, stored in the metadata database.

use kafka_auth::authz::Operation;
use kafka_meta::{Committed, MetaError, OffsetCommit, StoredOffset};
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::ids::{GroupId, InstanceId, MemberId, PartitionId, TopicId, TopicName};
use kafka_wire::messages::offset_commit_response::{
    OffsetCommitResponsePartition, OffsetCommitResponseTopic,
};
use kafka_wire::messages::{OffsetCommitRequest, OffsetCommitResponse};

use crate::groups::{Answer, CommitRequest, Req};
use crate::{Broker, Caller, Outcome, blocking, clock};

/// Kafka's `offset.metadata.max.bytes`: the longest text a commit may carry.
const MAX_METADATA: usize = 4096;

/// The first version of OffsetCommit that names its topics by id (KIP-848), and is answered with ids.
const BY_ID: i16 = 10;

/// The version whose offsets Kafka keeps in ZooKeeper, apart from every other commit.
const ZOOKEEPER: i16 = 0;

/// How long an offset committed again unchanged keeps its stored commit time instead of being written (and flushed)
/// again (`MetaStore::commit_offsets_unless_held`): a minute, or a tenth of how long offsets are kept when that is
/// shorter, so a commit time that lags never lets an offset a client keeps committing expire.
fn held_for(retention: std::time::Duration) -> i64 {
    const MINUTE_MS: i64 = 60_000;
    if retention.is_zero() {
        return MINUTE_MS;
    }
    i64::try_from(retention.as_millis() / 10).map_or(MINUTE_MS, |ms| ms.min(MINUTE_MS))
}

mod zookeeper;

impl<F: Fs> Broker<F> {
    /// Stores what each partition of the request says its group has reached.
    ///
    /// The caller must be allowed to READ the group (30 for every partition if not), and each topic it commits to (29 for
    /// that topic's partitions): before the topic is looked for, and before the group is asked about the committer.
    ///
    /// From v10 a topic is named by id: each id is turned into the name it stands for first, and one that stands for
    /// nothing is UNKNOWN_TOPIC_ID (100) on each of its partitions, before its READ is asked, and is not put to the group
    /// (`KafkaApis.handleOffsetCommitRequest`). The answer names each topic by the id it was asked by.
    pub(crate) async fn offset_commit(
        &self,
        version: i16,
        caller: &Caller,
        mut req: OffsetCommitRequest,
    ) -> Outcome {
        if version >= BY_ID {
            self.commit_names_by_id(&mut req);
        }
        if version == ZOOKEEPER {
            return self.offset_commit_v0(caller, &req);
        }
        let req = &req;
        let unknown_id = |name: &str| version >= BY_ID && name.is_empty();
        let guard = self.guard(caller);
        if !guard.group(Operation::Read, req.group_id.as_str()) {
            return blanket(req, ResponseError::GroupAuthorizationFailed);
        }
        // An id that stands for no topic is answered before any right is asked, and is left out like a barred topic.
        let barred = |name: &str| unknown_id(name) || !guard.topic(Operation::Read, name);
        let fence = self.fence_unless_barred(version, req, &barred).await;
        let mut resp = OffsetCommitResponse::default();
        // Each partition is judged; those that pass are stored together in one write.
        let mut to_store: Vec<OffsetCommit> = Vec::new();
        let mut slots: Vec<(usize, usize)> = Vec::new();
        // The partitions `KafkaApis` answers itself (an unknown id, a barred topic, a topic or partition that does not
        // exist), which come first in its answer.
        let mut early: Vec<(usize, usize)> = Vec::new();
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
                let (refusal, by_kafka_apis) = judge(
                    unknown_id(topic.name.as_str()),
                    refused_topic,
                    known.is_some(),
                    fence,
                    metadata.len(),
                );
                if by_kafka_apis {
                    early.push((t, p));
                }
                // v1 lets the client say when it committed (KIP-32's era): the time the offset's retention counts from.
                let committed_ms = if version == 1 && partition.commit_timestamp != -1 {
                    partition.commit_timestamp
                } else {
                    now
                };
                match to_be_stored(refusal, name.as_ref(), partition, metadata, committed_ms) {
                    Ok(commit) => {
                        to_store.push(commit);
                        slots.push((t, p));
                    }
                    Err(e) => answer.error_code = e.code(),
                }
                out.partitions.push(answer);
            }
            resp.topics.push(out);
        }
        if !to_store.is_empty() {
            self.store(req, &to_store, &slots, &mut resp);
        }
        Outcome::Respond(kafka_wire::api::Response::OffsetCommit(kafka_order_of(
            resp, &early,
        )))
    }

    /// Gives each topic of a v10 request the name its id stands for (v10 has no name on the wire), or none.
    fn commit_names_by_id(&self, req: &mut OffsetCommitRequest) {
        for t in &mut req.topics {
            let found = TopicId::from_uuid(t.topic_id).and_then(|id| self.meta.topic_by_id(id));
            t.name =
                kafka_wire::messages::TopicName(found.map_or_else(StrBytes::default, |topic| {
                    StrBytes::from_string(topic.name.as_ref().to_owned())
                }));
        }
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
            self.commit_fence(version, req, barred).await
        }
    }

    /// May this group's offsets be written by this committer? `None`: yes.
    async fn commit_fence(
        &self,
        version: i16,
        req: &OffsetCommitRequest,
        barred: &impl Fn(&str) -> bool,
    ) -> Option<ResponseError> {
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
            partitions: self.asked_of_the_group(req, barred),
        };
        self.ask_group_to_commit(&group, check).await
    }

    /// The partitions Kafka's group coordinator would be asked to validate, by topic id: `KafkaApis` answers a barred
    /// topic, a topic or partition that does not exist, itself, and the coordinator skips one whose metadata is too
    /// large before it validates the rest.
    fn asked_of_the_group(
        &self,
        req: &OffsetCommitRequest,
        barred: &impl Fn(&str) -> bool,
    ) -> Vec<(kafka_wire::Uuid, i32)> {
        let mut asked = Vec::new();
        for t in req.topics.iter().filter(|t| !barred(t.name.as_str())) {
            let Some(topic) = TopicName::parse(t.name.as_str())
                .ok()
                .and_then(|n| self.meta.topic(&n))
            else {
                continue;
            };
            asked.extend(
                t.partitions
                    .iter()
                    .filter(|p| {
                        p.partition_index >= 0 && p.partition_index < topic.partitions.get()
                    })
                    .filter(|p| {
                        p.committed_metadata
                            .as_ref()
                            .is_none_or(|m| m.len() <= MAX_METADATA)
                    })
                    .map(|p| (topic.id.to_uuid(), p.partition_index)),
            );
        }
        asked
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
        let within = held_for(self.config.offsets_retention);
        let result = blocking::run(|| {
            self.meta
                .commit_offsets_unless_held(&group, commits, within)
        });
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

/// The answer in the order Kafka's `OffsetCommitResponse.Builder` gives it: a topic appears where its first partition is
/// added, and the partitions `KafkaApis` answered itself (`early`) are added before the group's answer is merged in.
/// Kafka 4.4.0 answers a request for partitions 0, 1 and 2 of a two-partition topic with 2 first (`tools/wire-tail-oracle`,
/// `offsets-10`); clients find each partition by its index, so only the order changes.
fn kafka_order_of(
    mut resp: OffsetCommitResponse,
    early: &[(usize, usize)],
) -> OffsetCommitResponse {
    if early.is_empty() {
        return resp;
    }
    let asked = std::mem::take(&mut resp.topics);
    let mut topics: Vec<OffsetCommitResponseTopic> = Vec::new();
    // For each topic of the request, where it is in the new answer once it has a partition there.
    let mut placed: Vec<Option<usize>> = vec![None; asked.len()];
    let mut place = |topics: &mut Vec<OffsetCommitResponseTopic>,
                     t: usize,
                     from: &OffsetCommitResponseTopic,
                     part: OffsetCommitResponsePartition| {
        let Some(slot) = placed.get_mut(t) else {
            return;
        };
        let at = *slot.get_or_insert_with(|| {
            let mut topic = from.clone();
            topic.partitions.clear();
            topics.push(topic);
            topics.len() - 1
        });
        if let Some(topic) = topics.get_mut(at) {
            topic.partitions.push(part);
        }
    };
    for (t, topic) in asked.iter().enumerate() {
        for (p, part) in topic.partitions.iter().enumerate() {
            if early.contains(&(t, p)) {
                place(&mut topics, t, topic, part.clone());
            }
        }
    }
    for (t, topic) in asked.iter().enumerate() {
        for (p, part) in topic.partitions.iter().enumerate() {
            if !early.contains(&(t, p)) {
                place(&mut topics, t, topic, part.clone());
            }
        }
    }
    resp.topics = topics;
    resp
}

/// The commit to store for a partition that passed, or why it is not stored.
fn to_be_stored(
    refusal: Option<ResponseError>,
    name: Option<&TopicName>,
    partition: &kafka_wire::messages::offset_commit_request::OffsetCommitRequestPartition,
    metadata: &str,
    committed_ms: i64,
) -> Result<OffsetCommit, ResponseError> {
    if let Some(e) = refusal {
        return Err(e);
    }
    match (name, PartitionId::new(partition.partition_index)) {
        (Some(name), Some(id)) => Ok(OffsetCommit {
            topic: name.clone(),
            partition: id,
            offset: StoredOffset {
                offset: partition.committed_offset,
                leader_epoch: partition.committed_leader_epoch,
                metadata: metadata.to_owned(),
                committed_ms,
            },
        }),
        _ => Err(ResponseError::UnknownTopicOrPartition),
    }
}

/// What a partition is answered, if not stored, and whether `KafkaApis` answers it itself (before the group is asked, and
/// first in its answer): an unknown id, a barred topic, a topic or partition that does not exist. The group's answer
/// (`fence`) is for the rest, then the metadata's size.
fn judge(
    unknown_id: bool,
    barred: bool,
    known: bool,
    fence: Option<ResponseError>,
    metadata_len: usize,
) -> (Option<ResponseError>, bool) {
    if unknown_id {
        (Some(ResponseError::UnknownTopicId), true)
    } else if barred {
        (Some(ResponseError::TopicAuthorizationFailed), true)
    } else if !known {
        (Some(ResponseError::UnknownTopicOrPartition), true)
    } else if fence.is_some() {
        (fence, false)
    } else if metadata_len > MAX_METADATA {
        (Some(ResponseError::OffsetMetadataTooLarge), false)
    } else {
        (None, false)
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
