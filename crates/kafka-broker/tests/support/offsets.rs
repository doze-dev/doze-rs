//! Requests for OffsetCommit and OffsetFetch.
#![allow(
    unreachable_pub,
    dead_code,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::Rig;
use bytes::Bytes;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::GroupId;

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

/// One partition of an OffsetCommit: (topic, partition, offset, metadata).
pub type Commit<'a> = (&'a str, i32, i64, Option<&'a str>);

impl Rig {
    pub async fn commit_offsets(
        &self,
        version: i16,
        group: &str,
        member: &str,
        generation: i32,
        commits: &[Commit<'_>],
    ) -> kafka_wire::messages::OffsetCommitResponse {
        use kafka_wire::messages::offset_commit_request::{
            OffsetCommitRequestPartition, OffsetCommitRequestTopic,
        };
        let mut req = kafka_wire::messages::OffsetCommitRequest::default();
        req.group_id = GroupId(text(group));
        req.member_id = text(member);
        req.generation_id_or_member_epoch = generation;
        for (name, partition, offset, metadata) in commits {
            let mut p = OffsetCommitRequestPartition::default();
            p.partition_index = *partition;
            p.committed_offset = *offset;
            p.committed_metadata = metadata.map(text);
            match req.topics.iter_mut().find(|t| t.name.as_str() == *name) {
                Some(t) => t.partitions.push(p),
                None => {
                    let mut t = OffsetCommitRequestTopic::default();
                    t.name = kafka_wire::messages::TopicName(text(name));
                    t.partitions.push(p);
                    req.topics.push(t);
                }
            }
        }
        match self.ask(version, Request::OffsetCommit(req)).await {
            Response::OffsetCommit(r) => r,
            other => panic!("not OffsetCommit: {other:?}"),
        }
    }

    /// What the group has committed: `asked` is the topics and partitions, or `None` for all there are. Returns
    /// (topic, partition, offset, metadata) in the order answered; `batched` uses the v8 form.
    pub async fn fetch_offsets(
        &self,
        batched: bool,
        group: &str,
        asked: Option<&[(&str, &[i32])]>,
    ) -> Vec<(String, i32, i64, String)> {
        use kafka_wire::messages::offset_fetch_request::{
            OffsetFetchRequestGroup, OffsetFetchRequestTopic, OffsetFetchRequestTopics,
        };
        let name = |n: &str| kafka_wire::messages::TopicName(text(n));
        let mut req = kafka_wire::messages::OffsetFetchRequest::default();
        if batched {
            let mut g = OffsetFetchRequestGroup::default();
            g.group_id = GroupId(text(group));
            g.topics = asked.map(|ts| {
                ts.iter()
                    .map(|(n, ps)| {
                        let mut t = OffsetFetchRequestTopics::default();
                        t.name = name(n);
                        t.partition_indexes = ps.to_vec();
                        t
                    })
                    .collect()
            });
            req.groups.push(g);
        } else {
            req.group_id = GroupId(text(group));
            req.topics = asked.map(|ts| {
                ts.iter()
                    .map(|(n, ps)| {
                        let mut t = OffsetFetchRequestTopic::default();
                        t.name = name(n);
                        t.partition_indexes = ps.to_vec();
                        t
                    })
                    .collect()
            });
        }
        let version = if batched { 8 } else { 7 };
        let Response::OffsetFetch(r) = self.ask(version, Request::OffsetFetch(req)).await else {
            panic!("not OffsetFetch");
        };
        let mut out = Vec::new();
        for t in &r.topics {
            for p in &t.partitions {
                assert_eq!(p.error_code, 0);
                out.push((
                    t.name.to_string(),
                    p.partition_index,
                    p.committed_offset,
                    p.metadata
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_default(),
                ));
            }
        }
        for g in &r.groups {
            for t in &g.topics {
                for p in &t.partitions {
                    assert_eq!(p.error_code, 0);
                    out.push((
                        t.name.to_string(),
                        p.partition_index,
                        p.committed_offset,
                        p.metadata
                            .as_ref()
                            .map(ToString::to_string)
                            .unwrap_or_default(),
                    ));
                }
            }
        }
        out
    }
}
