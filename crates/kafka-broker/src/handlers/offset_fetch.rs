//! OffsetFetch: what a group has committed, or -1 where it has not.

use std::collections::HashSet;

use kafka_meta::GroupOffset;
use kafka_storage::fs::Fs;
use kafka_wire::StrBytes;
use kafka_wire::ids::{GroupId, TopicId, TopicName};
use kafka_wire::messages::offset_fetch_response::{
    OffsetFetchResponseGroup, OffsetFetchResponsePartition, OffsetFetchResponsePartitions,
    OffsetFetchResponseTopic, OffsetFetchResponseTopics,
};
use kafka_wire::messages::{OffsetFetchRequest, OffsetFetchResponse, TopicName as WireName};

use kafka_auth::authz::Operation;

use crate::{Broker, Caller, Outcome, blocking};

mod check;
mod guard;

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

/// The first version of OffsetFetch that names its topics by id (KIP-848), and is answered with ids.
const BY_ID: i16 = 10;

/// The version that reads the offsets Kafka keeps in ZooKeeper.
const ZOOKEEPER: i16 = 0;

/// What was asked of a group: every offset it has, or these partitions of these topics.
type Asked<'a> = Option<Vec<(&'a WireName, &'a [i32])>>;

impl<F: Fs> Broker<F> {
    /// What each group asked about has committed.
    ///
    /// The caller must be allowed to DESCRIBE the group (30, in the shape of the version), and sees the topics it may
    /// DESCRIBE (see `guard.rs`).
    ///
    /// From v10 a topic is named by id: each id is turned into the name it stands for first, and one that stands for none
    /// is 100 on each partition asked. The answer names each topic by its id; asked for every offset, a topic with no id
    /// (one deleted since its offsets were committed) is left out, as Kafka cannot write it.
    pub(crate) async fn offset_fetch(
        &self,
        version: i16,
        caller: &Caller,
        mut req: OffsetFetchRequest,
    ) -> Outcome {
        let by_id = version >= BY_ID;
        // The ids asked for that stand for no topic, in the order asked: the answer gives them back as they came.
        let mut unknown_ids = if by_id {
            self.fetch_names_by_id(&mut req)
        } else {
            Vec::new()
        }
        .into_iter();
        let req = &req;
        let guard = self.guard(caller);
        let mut resp = OffsetFetchResponse::default();
        if req.groups.is_empty() {
            self.answer_one_group(version, &guard, req, &mut resp).await;
        } else {
            for g in &req.groups {
                let mut group = OffsetFetchResponseGroup::default();
                group.group_id = g.group_id.clone();
                let member = g.member_id.as_ref().map(|m| m.as_str());
                if !guard.group(Operation::Describe, g.group_id.as_str()) {
                    group.error_code = kafka_wire::ResponseError::GroupAuthorizationFailed.code();
                    resp.groups.push(group);
                    continue;
                }
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
                for (name, found) in
                    self.answer_guarded(&guard, g.group_id.as_str(), asked, &pending, by_id)
                {
                    let mut topic = OffsetFetchResponseTopics::default();
                    if by_id {
                        let id = if name.is_empty() {
                            unknown_ids.next()
                        } else {
                            self.fetched_topic_id(&name)
                        };
                        let Some(id) = id else { continue };
                        topic.topic_id = id;
                    }
                    topic.name = name;
                    topic.partitions = found.into_iter().map(batched).collect();
                    group.topics.push(topic);
                }
                resp.groups.push(group);
            }
        }
        Outcome::Respond(kafka_wire::api::Response::OffsetFetch(resp))
    }

    /// Versions 0 to 7: one group, named at the top.
    async fn answer_one_group(
        &self,
        version: i16,
        guard: &crate::handlers::group_guard::Guard<'_>,
        req: &OffsetFetchRequest,
        resp: &mut OffsetFetchResponse,
    ) {
        let asked: Asked<'_> = req.topics.as_ref().map(|ts| {
            ts.iter()
                .map(|t| (&t.name, t.partition_indexes.as_slice()))
                .collect()
        });
        if !guard.group(Operation::Describe, req.group_id.as_str()) {
            guard::refuse_old(version, &asked, resp);
            return;
        }
        if version == ZOOKEEPER {
            self.answer_from_zookeeper(guard, req, resp);
            return;
        }
        let pending = self
            .pending_if(req.require_stable, req.group_id.as_str())
            .await;
        for (name, found) in
            self.answer_guarded(guard, req.group_id.as_str(), asked, &pending, false)
        {
            let mut topic = OffsetFetchResponseTopic::default();
            topic.name = name;
            topic.partitions = found.into_iter().map(single).collect();
            resp.topics.push(topic);
        }
    }

    /// OffsetFetch v0, which Kafka 1.1 and 2.8 read from ZooKeeper (`KafkaApis.handleOffsetFetchRequest`): what
    /// OffsetCommit v0 stored there, and nothing any later version committed. Each partition of a topic the caller may not
    /// describe is 29; of a topic or partition that does not exist, or that has nothing there, 3 (`UNKNOWN_PARTITION`);
    /// always with no metadata. Kafka 3 has no ZooKeeper: every partition asked is UNSUPPORTED_VERSION.
    fn answer_from_zookeeper(
        &self,
        guard: &crate::handlers::group_guard::Guard<'_>,
        req: &OffsetFetchRequest,
        resp: &mut OffsetFetchResponse,
    ) {
        let zookeeper = self.config.kafka_major.get() <= 2;
        let group = GroupId::parse(req.group_id.as_str()).ok();
        for t in req.topics.iter().flatten() {
            let mut topic = OffsetFetchResponseTopic::default();
            topic.name = t.name.clone();
            let barred = !guard.topic(Operation::Describe, t.name.as_str());
            let name = TopicName::parse(t.name.as_str()).ok();
            for &index in &t.partition_indexes {
                let error = |code: kafka_wire::ResponseError| Found {
                    error: code.code(),
                    ..Found::none(index)
                };
                let found = if !zookeeper {
                    error(kafka_wire::ResponseError::UnsupportedVersion)
                } else if barred {
                    error(kafka_wire::ResponseError::TopicAuthorizationFailed)
                } else {
                    let stored = name
                        .as_ref()
                        .zip(group.as_ref())
                        .zip(kafka_wire::ids::PartitionId::new(index))
                        .filter(|((n, _), id)| {
                            self.meta
                                .topic(n)
                                .is_some_and(|t| id.get() < t.partitions.get())
                        })
                        .and_then(|((n, g), id)| {
                            blocking::run(|| self.meta.zk_offset(g, n, id)).unwrap_or_else(|e| {
                                tracing::error!(error = %e, "reading v0 offsets failed");
                                None
                            })
                        });
                    match stored {
                        Some(offset) => Found {
                            offset,
                            ..Found::none(index)
                        },
                        None => error(kafka_wire::ResponseError::UnknownTopicOrPartition),
                    }
                };
                topic.partitions.push(single(found));
            }
            resp.topics.push(topic);
        }
    }

    /// Gives each topic of a v10 request the name its id stands for (v10 has no name on the wire), or none; returns the ids
    /// that stand for none, in the order asked.
    fn fetch_names_by_id(&self, req: &mut OffsetFetchRequest) -> Vec<kafka_wire::Uuid> {
        let mut unknown = Vec::new();
        for t in req
            .groups
            .iter_mut()
            .flat_map(|g| g.topics.iter_mut().flatten())
        {
            let found = TopicId::from_uuid(t.topic_id).and_then(|id| self.meta.topic_by_id(id));
            if found.is_none() {
                unknown.push(t.topic_id);
            }
            t.name = WireName(found.map_or_else(StrBytes::default, |topic| {
                StrBytes::from_string(topic.name.as_ref().to_owned())
            }));
        }
        unknown
    }

    /// The id of the topic named `name`, if there is one.
    fn fetched_topic_id(&self, name: &WireName) -> Option<kafka_wire::Uuid> {
        TopicName::parse(name.as_str())
            .ok()
            .and_then(|n| self.meta.topic(&n))
            .map(|t| t.id.to_uuid())
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
