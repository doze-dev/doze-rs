//! TxnOffsetCommit: consumer offsets staged with a transaction, applied if it commits.

use kafka_auth::authz::Operation;
use kafka_meta::{TxnOffset, TxnState, TxnVersion};
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::StrBytes;
use kafka_wire::api::Response;
use kafka_wire::ids::{GroupId, InstanceId, MemberId, TopicId, TopicName};
use kafka_wire::messages::txn_offset_commit_request::TxnOffsetCommitRequestTopic;
use kafka_wire::messages::txn_offset_commit_response::{
    TxnOffsetCommitResponsePartition, TxnOffsetCommitResponseTopic,
};
use kafka_wire::messages::{TxnOffsetCommitRequest, TxnOffsetCommitResponse};

use crate::groups::CommitRequest;
use crate::handlers::group_guard::Guard;
use crate::txn::rules;
use crate::{Broker, Caller, Outcome};

/// The first version of TxnOffsetCommit that adds its group to the transaction (KIP-890's second part: a client of it sends
/// no AddOffsetsToTxn).
const ADDS_THE_GROUP: i16 = 5;

/// The first version of TxnOffsetCommit that names its topics by id (KIP-516), and is answered with ids.
const BY_ID: i16 = 6;

impl<F: Fs> Broker<F> {
    /// The caller must be allowed to WRITE the transactional id (53 on every partition of the request), then to READ the
    /// group (30 on every partition), and each topic is READ in turn (29 on its partitions): in that order, and before the
    /// topic is looked for.
    ///
    /// From v6 a topic is named by id: each id is turned into the name it stands for before the topics are judged, and
    /// one that stands for nothing is UNKNOWN_TOPIC_ID on each of its partitions, before its READ is asked
    /// (`KafkaApis.handleTxnOffsetCommitRequest`). The answer names each topic by the id it was asked by.
    pub(crate) async fn txn_offset_commit(
        &self,
        version: i16,
        caller: &Caller,
        mut req: TxnOffsetCommitRequest,
    ) -> Outcome {
        if version >= BY_ID {
            for t in &mut req.topics {
                // (v6 has no name on the wire: the one the id stands for, or none.)
                let found = TopicId::from_uuid(t.topic_id).and_then(|id| self.meta.topic_by_id(id));
                t.name = kafka_wire::messages::TopicName(
                    found.map_or_else(StrBytes::default, |topic| {
                        StrBytes::from_string(topic.name.as_ref().to_owned())
                    }),
                );
            }
        }
        let unknown_id = |t: &TxnOffsetCommitRequestTopic| version >= BY_ID && t.name.0.is_empty();
        let req = &req;
        let guard = self.guard(caller);
        let whole = if !guard.txn(Operation::Write, req.transactional_id.0.as_str()) {
            Some(E::TransactionalIdAuthorizationFailed)
        } else if !guard.group(Operation::Read, req.group_id.0.as_str()) {
            Some(E::GroupAuthorizationFailed)
        } else {
            None
        };
        let code = match whole {
            Some(_) => whole,
            None => self.stage_offsets(version, &guard, req, unknown_id).await,
        };
        let mut resp = TxnOffsetCommitResponse::default();
        for t in &req.topics {
            let mut rt = TxnOffsetCommitResponseTopic::default();
            rt.name = t.name.clone();
            rt.topic_id = t.topic_id;
            let unknown_id = unknown_id(t);
            let barred = !unknown_id && !guard.topic(Operation::Read, t.name.0.as_str());
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
                    Some(c) if whole.is_some() => c.code(),
                    _ if unknown_id => E::UnknownTopicId.code(),
                    _ if barred => E::TopicAuthorizationFailed.code(),
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
    async fn stage_offsets(
        &self,
        version: i16,
        guard: &Guard<'_>,
        req: &TxnOffsetCommitRequest,
        unknown_id: impl Fn(&TxnOffsetCommitRequestTopic) -> bool + Copy,
    ) -> Option<E> {
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
        // The group must have been added to the open transaction (AddOffsetsToTxn), or from v5 (KIP-890's second part) it
        // is added here, as Kafka's group coordinator asks the transaction coordinator to before it stages the offsets
        // (`ADD_PARTITION`), whatever the committer's place in the group turns out to be.
        let added = e.rec.state == TxnState::Ongoing && rules::has_group(&e.rec, group.as_str());
        if !added {
            if version < ADDS_THE_GROUP {
                return Some(rules::not_added(version, 4));
            }
            let now = crate::clock::now().get();
            let stored = self
                .update_txn(&mut e, |rec| {
                    rules::add_group(rec, group.as_str(), TxnVersion::EpochBump, now);
                })
                .await;
            if let Err(code) = stored {
                return Some(code);
            }
        }
        let committer = CommitRequest {
            // (The version only matters to OffsetCommit, which older versions cannot name a member epoch with.)
            version: 9,
            member_id: MemberId::new(req.member_id.as_str()),
            instance: req
                .group_instance_id
                .as_ref()
                .map(|i| InstanceId::new(i.as_str())),
            generation: req.generation_id_or_member_epoch,
            transactional: true,
            // Those that exist and may be read: `KafkaApis` answers the others before the coordinator is asked.
            partitions: self
                .offsets_of(guard, req, group.as_str(), unknown_id)
                .iter()
                .filter_map(|o| {
                    let topic = TopicName::parse(&o.topic)
                        .ok()
                        .and_then(|n| self.meta.topic(&n))?;
                    Some((topic.id.to_uuid(), o.partition))
                })
                .collect(),
        };
        if let Some(code) = self.ask_group_to_commit(&group, committer).await {
            return Some(code);
        }
        let staged = self.offsets_of(guard, req, group.as_str(), unknown_id);
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

    /// The offsets of the request for partitions that exist (of a topic it names by an id that stands for one, from v6).
    fn offsets_of(
        &self,
        guard: &Guard<'_>,
        req: &TxnOffsetCommitRequest,
        group: &str,
        unknown_id: impl Fn(&TxnOffsetCommitRequestTopic) -> bool,
    ) -> Vec<TxnOffset> {
        let group_name = group.to_owned();
        req.topics
            .iter()
            .filter(|t| !unknown_id(t) && guard.topic(Operation::Read, t.name.0.as_str()))
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
