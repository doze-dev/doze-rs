//! TxnOffsetCommit: consumer offsets staged with a transaction, applied if it commits.

use kafka_meta::TxnOffset;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::api::Response;
use kafka_wire::ids::{GroupId, TopicName};
use kafka_wire::messages::txn_offset_commit_response::{
    TxnOffsetCommitResponsePartition, TxnOffsetCommitResponseTopic,
};
use kafka_wire::messages::{TxnOffsetCommitRequest, TxnOffsetCommitResponse};

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
                rp.error_code = match code {
                    Some(c) => c.code(),
                    None if known => 0,
                    None => E::UnknownTopicOrPartition.code(),
                };
                rt.partitions.push(rp);
            }
            resp.topics.push(rt);
        }
        Outcome::Respond(Response::TxnOffsetCommit(resp))
    }

    /// Stages the offsets of the partitions that exist. `Some`: why none were.
    async fn stage_offsets(&self, req: &TxnOffsetCommitRequest) -> Option<E> {
        let id = req.transactional_id.0.as_str();
        if id.is_empty() {
            return Some(E::InvalidRequest);
        }
        let Ok(group) = GroupId::parse(req.group_id.0.as_str()) else {
            return Some(E::InvalidGroupId);
        };
        let Some(entry) = self.txns.get(id) else {
            return Some(E::InvalidProducerIdMapping);
        };
        let mut e = entry.write().await;
        if let Some(code) = rules::owns(&e.rec, req.producer_id.0, req.producer_epoch) {
            return Some(code);
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
        let group_name = group.to_string();
        let staged: Vec<TxnOffset> = req
            .topics
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
            .collect();
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
}
