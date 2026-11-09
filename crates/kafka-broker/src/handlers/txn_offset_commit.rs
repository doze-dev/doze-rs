//! TxnOffsetCommit: consumer offsets staged with a transaction, applied if it commits.

use kafka_meta::TxnOffset;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::api::Response;
use kafka_wire::ids::{GroupId, InstanceId, MemberId, TopicName};
use kafka_wire::messages::txn_offset_commit_response::{
    TxnOffsetCommitResponsePartition, TxnOffsetCommitResponseTopic,
};
use kafka_wire::messages::{TxnOffsetCommitRequest, TxnOffsetCommitResponse};

use crate::groups::CommitRequest;
use crate::txn::rules;
use crate::{Broker, Outcome};

impl<F: Fs> Broker<F> {
    pub(crate) async fn txn_offset_commit(&self, req: &TxnOffsetCommitRequest) -> Outcome {
        let code = self.stage_offsets(req).await;
        let mut resp = TxnOffsetCommitResponse::default();
        for t in &req.topics {
            let mut rt = TxnOffsetCommitResponseTopic::default();
            rt.name = t.name.clone();
            for p in &t.partitions {
                let mut rp = TxnOffsetCommitResponsePartition::default();
                rp.partition_index = p.partition_index;
                let known = TopicName::parse(&t.name.0)
                    .ok()
                    .and_then(|n| self.meta.topic(&n))
                    .is_some_and(|t| {
                        p.partition_index >= 0 && p.partition_index < t.partitions.get()
                    });
                // A partition that is not there is said so whatever else is wrong; the others share the request's answer.
                rp.error_code = match code {
                    _ if !known => E::UnknownTopicOrPartition.code(),
                    Some(c) => c.code(),
                    None => 0,
                };
                rt.partitions.push(rp);
            }
            resp.topics.push(rt);
        }
        Outcome::Respond(Response::TxnOffsetCommit(resp))
    }

    /// Stages the offsets of the partitions that exist. `Some`: why none were.
    async fn stage_offsets(&self, req: &TxnOffsetCommitRequest) -> Option<E> {
        // In Kafka's order: the group's id, then the producer's right to write to the transaction (an id nobody owns,
        // including the empty one, is no producer's), then the committer's place in the group.
        let id = req.transactional_id.0.as_str();
        let Ok(group) = GroupId::parse(req.group_id.0.as_str()) else {
            return Some(E::InvalidGroupId);
        };
        let Some(mut e) = self.txns.lock(id).await else {
            return Some(E::InvalidProducerIdMapping);
        };
        // (A producer that has been replaced is told the epoch is invalid, as a partition tells it.)
        if let Some(code) = rules::owns(&e.rec, req.producer_id.0, req.producer_epoch) {
            return Some(rules::fenced_as(code, 0, i16::MAX));
        }
        if let Some(code) = rules::can_add(&e.rec) {
            return Some(code);
        }
        // The group must have been added to the open transaction.
        if e.rec.state != kafka_meta::TxnState::Ongoing
            || !e.rec.groups.iter().any(|g| g == group.as_str())
        {
            return Some(E::InvalidTxnState);
        }
        let committer = CommitRequest {
            // (The version only matters to OffsetCommit, which older versions cannot name a member epoch with.)
            version: 9,
            member_id: MemberId::new(req.member_id.as_str()),
            instance: req
                .group_instance_id
                .as_ref()
                .map(|i| InstanceId::new(i.as_str())),
            generation: req.generation_id,
            transactional: true,
        };
        if let Some(code) = self.ask_group_to_commit(&group, committer).await {
            return Some(code);
        }
        let staged = self.offsets_of(req, group.as_str());
        if staged.is_empty() {
            return None;
        }
        self.update_txn(&mut e, |rec| {
            for o in staged {
                rec.offsets.retain(|x| {
                    !(x.group == o.group && x.topic == o.topic && x.partition == o.partition)
                });
                rec.offsets.push(o);
            }
        })
        .await
        .err()
    }

    /// The offsets of the request for partitions that exist.
    fn offsets_of(&self, req: &TxnOffsetCommitRequest, group: &str) -> Vec<TxnOffset> {
        let group_name = group.to_owned();
        req.topics
            .iter()
            .flat_map(|t| {
                let size = TopicName::parse(&t.name.0)
                    .ok()
                    .and_then(|n| self.meta.topic(&n))
                    .map(|topic| topic.partitions.get());
                let group_name = group_name.clone();
                t.partitions
                    .iter()
                    .filter(move |p| {
                        size.is_some_and(|n| p.partition_index >= 0 && p.partition_index < n)
                    })
                    .map(move |p| TxnOffset {
                        group: group_name.clone(),
                        topic: t.name.0.to_string(),
                        partition: p.partition_index,
                        offset: p.committed_offset,
                        leader_epoch: p.committed_leader_epoch,
                        metadata: p
                            .committed_metadata
                            .as_ref()
                            .map_or_else(String::new, |m| m.to_string()),
                    })
            })
            .collect()
    }
}
