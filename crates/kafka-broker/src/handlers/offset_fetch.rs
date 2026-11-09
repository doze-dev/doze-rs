//! OffsetFetch: what a group has committed, or -1 where it has not.

use std::collections::HashSet;

use kafka_meta::GroupOffset;
use kafka_storage::fs::Fs;
use kafka_wire::StrBytes;
use kafka_wire::ids::GroupId;
use kafka_wire::messages::offset_fetch_response::{
    OffsetFetchResponseGroup, OffsetFetchResponsePartition, OffsetFetchResponsePartitions,
    OffsetFetchResponseTopic, OffsetFetchResponseTopics,
};
use kafka_wire::messages::{OffsetFetchRequest, OffsetFetchResponse, TopicName as WireName};

use crate::{Broker, Outcome, blocking};

mod check;

/// One answer: the offset, or -1 with nothing.
struct Found {
    partition: i32,
    error: i16,
    offset: i64,
    leader_epoch: i32,
    metadata: String,
}

impl Found {
    fn unstable(partition: i32) -> Self {
        Self {
            error: UNSTABLE_OFFSET_COMMIT,
            ..Self::none(partition)
        }
    }

    fn none(partition: i32) -> Self {
        Self {
            partition,
            error: 0,
            offset: -1,
            leader_epoch: -1,
            metadata: String::new(),
        }
    }
}

/// Offsets that a transaction is still deciding: the partition has none to give yet.
const UNSTABLE_OFFSET_COMMIT: i16 = 88;

/// What was asked of a group: every offset it has, or these partitions of these topics.
type Asked<'a> = Option<Vec<(&'a WireName, &'a [i32])>>;

impl<F: Fs> Broker<F> {
    pub(crate) async fn offset_fetch(&self, req: &OffsetFetchRequest) -> Outcome {
        let mut resp = OffsetFetchResponse::default();
        if req.groups.is_empty() {
            // Versions 0 to 7: one group, named at the top.
            let asked: Asked<'_> = req.topics.as_ref().map(|ts| {
                ts.iter()
                    .map(|t| (&t.name, t.partition_indexes.as_slice()))
                    .collect()
            });
            let pending = self
                .pending_if(req.require_stable, req.group_id.as_str())
                .await;
            for (name, found) in self.answer(req.group_id.as_str(), asked, &pending) {
                let mut topic = OffsetFetchResponseTopic::default();
                topic.name = name;
                topic.partitions = found.into_iter().map(single).collect();
                resp.topics.push(topic);
            }
        } else {
            for g in &req.groups {
                let mut group = OffsetFetchResponseGroup::default();
                group.group_id = g.group_id.clone();
                let member = g.member_id.as_ref().map(|m| m.as_str());
                if let Some(e) = self
                    .member_refusal(g.group_id.as_str(), member, g.member_epoch)
                    .await
                {
                    group.error_code = e.code();
                    resp.groups.push(group);
                    continue;
                }
                let pending = self
                    .pending_if(req.require_stable, g.group_id.as_str())
                    .await;
                let asked: Asked<'_> = g.topics.as_ref().map(|ts| {
                    ts.iter()
                        .map(|t| (&t.name, t.partition_indexes.as_slice()))
                        .collect()
                });
                for (name, found) in self.answer(g.group_id.as_str(), asked, &pending) {
                    let mut topic = OffsetFetchResponseTopics::default();
                    topic.name = name;
                    topic.partitions = found.into_iter().map(batched).collect();
                    group.topics.push(topic);
                }
                resp.groups.push(group);
            }
        }
        Outcome::Respond(kafka_wire::api::Response::OffsetFetch(resp))
    }

    /// The offsets of `group`, by topic. A group, topic or partition that is not known has offset -1: not an error,
    /// so that a new consumer starts where its `auto.offset.reset` says.
    fn answer(
        &self,
        group: &str,
        asked: Asked<'_>,
        pending: &HashSet<(String, i32)>,
    ) -> Vec<(WireName, Vec<Found>)> {
        let committed: Vec<GroupOffset> = match GroupId::parse(group) {
            Ok(id) => blocking::run(|| self.meta.group_offsets(&id)).unwrap_or_else(|e| {
                tracing::error!(group, error = %e, "reading committed offsets failed");
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        let lookup = |topic: &str, partition: i32| {
            committed
                .iter()
                .find(|o| o.topic.as_str() == topic && o.partition.get() == partition)
        };
        let found = |o: &GroupOffset| {
            if pending.contains(&(o.topic.to_string(), o.partition.get())) {
                return Found::unstable(o.partition.get());
            }
            Found {
                partition: o.partition.get(),
                error: 0,
                offset: o.offset.offset,
                leader_epoch: o.offset.leader_epoch,
                metadata: o.offset.metadata.clone(),
            }
        };
        match asked {
            None => {
                let mut out: Vec<(WireName, Vec<Found>)> = Vec::new();
                for o in &committed {
                    let name = WireName(StrBytes::from_string(o.topic.to_string()));
                    match out.last_mut() {
                        Some((last, list)) if *last == name => list.push(found(o)),
                        _ => out.push((name, vec![found(o)])),
                    }
                }
                out
            }
            Some(topics) => topics
                .into_iter()
                .map(|(name, partitions)| {
                    let list = partitions
                        .iter()
                        .map(|p| {
                            lookup(name.as_str(), *p).map_or_else(
                                || {
                                    if pending.contains(&(name.to_string(), *p)) {
                                        Found::unstable(*p)
                                    } else {
                                        Found::none(*p)
                                    }
                                },
                                found,
                            )
                        })
                        .collect();
                    (name.clone(), list)
                })
                .collect(),
        }
    }
}

fn single(f: Found) -> OffsetFetchResponsePartition {
    let mut p = OffsetFetchResponsePartition::default();
    p.partition_index = f.partition;
    p.committed_offset = f.offset;
    p.committed_leader_epoch = f.leader_epoch;
    p.metadata = Some(StrBytes::from_string(f.metadata));
    p.error_code = f.error;
    p
}

fn batched(f: Found) -> OffsetFetchResponsePartitions {
    let mut p = OffsetFetchResponsePartitions::default();
    p.partition_index = f.partition;
    p.committed_offset = f.offset;
    p.committed_leader_epoch = f.leader_epoch;
    p.metadata = Some(StrBytes::from_string(f.metadata));
    p.error_code = f.error;
    p
}
