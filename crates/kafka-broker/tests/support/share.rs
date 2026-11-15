//! Requests of the share group APIs (KIP-932), as a share consumer and Kafka's admin tools send them.
#![allow(unreachable_pub, dead_code, clippy::unwrap_used, clippy::panic)]

use kafka_wire::StrBytes;
use kafka_wire::Uuid;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::alter_share_group_offsets_request::{
    AlterShareGroupOffsetsRequestPartition, AlterShareGroupOffsetsRequestTopic,
};
use kafka_wire::messages::delete_share_group_offsets_request::DeleteShareGroupOffsetsRequestTopic;
use kafka_wire::messages::describe_share_group_offsets_request::{
    DescribeShareGroupOffsetsRequestGroup, DescribeShareGroupOffsetsRequestTopic,
};
use kafka_wire::messages::incremental_alter_configs_request::{AlterConfigsResource, AlterableConfig};
use kafka_wire::messages::share_acknowledge_request::{AcknowledgePartition, AcknowledgeTopic};
use kafka_wire::messages::share_fetch_request::{AcknowledgementBatch, FetchPartition, FetchTopic};
use kafka_wire::messages::{
    AlterShareGroupOffsetsRequest, AlterShareGroupOffsetsResponse, DeleteShareGroupOffsetsRequest,
    DeleteShareGroupOffsetsResponse, DescribeShareGroupOffsetsRequest,
    DescribeShareGroupOffsetsResponse, GroupId, IncrementalAlterConfigsRequest, ShareAcknowledgeRequest,
    ShareAcknowledgeResponse, ShareFetchRequest, ShareFetchResponse, ShareGroupDescribeRequest,
    ShareGroupDescribeResponse, ShareGroupHeartbeatRequest, ShareGroupHeartbeatResponse,
};

use super::{Rig, wire_name};

pub fn heartbeat_request(group: &str, member: &str, epoch: i32, topics: Option<&[&str]>) -> ShareGroupHeartbeatRequest {
    let mut r = ShareGroupHeartbeatRequest::default();
    r.group_id = GroupId(StrBytes::from_string(group.to_owned()));
    r.member_id = StrBytes::from_string(member.to_owned());
    r.member_epoch = epoch;
    r.subscribed_topic_names = topics.map(|t| t.iter().map(|n| wire_name(n)).collect());
    r
}

/// A fetch of `parts` (topic id, partition, acknowledgement batches as (first, last, types)).
pub fn fetch_request(
    group: &str,
    member: &str,
    epoch: i32,
    parts: &[(Uuid, i32, Vec<(i64, i64, Vec<i8>)>)],
) -> ShareFetchRequest {
    let mut r = ShareFetchRequest::default();
    r.group_id = Some(GroupId(StrBytes::from_string(group.to_owned())));
    r.member_id = Some(StrBytes::from_string(member.to_owned()));
    r.share_session_epoch = epoch;
    r.max_wait_ms = 0;
    r.min_bytes = 1;
    r.max_bytes = 1 << 20;
    r.max_records = 500;
    r.batch_size = 500;
    for (topic, partition, acks) in parts {
        let i = match r.topics.iter().position(|t| t.topic_id == *topic) {
            Some(i) => i,
            None => {
                let mut t = FetchTopic::default();
                t.topic_id = *topic;
                r.topics.push(t);
                r.topics.len() - 1
            }
        };
        let mut p = FetchPartition::default();
        p.partition_index = *partition;
        p.acknowledgement_batches = acks
            .iter()
            .map(|(first, last, types)| {
                let mut b = AcknowledgementBatch::default();
                b.first_offset = *first;
                b.last_offset = *last;
                b.acknowledge_types = types.clone();
                b
            })
            .collect();
        r.topics[i].partitions.push(p);
    }
    r
}

pub fn ack_request(
    group: &str,
    member: &str,
    epoch: i32,
    parts: &[(Uuid, i32, Vec<(i64, i64, Vec<i8>)>)],
) -> ShareAcknowledgeRequest {
    let mut r = ShareAcknowledgeRequest::default();
    r.group_id = Some(GroupId(StrBytes::from_string(group.to_owned())));
    r.member_id = Some(StrBytes::from_string(member.to_owned()));
    r.share_session_epoch = epoch;
    for (topic, partition, acks) in parts {
        let mut t = AcknowledgeTopic::default();
        t.topic_id = *topic;
        let mut p = AcknowledgePartition::default();
        p.partition_index = *partition;
        p.acknowledgement_batches = acks
            .iter()
            .map(|(first, last, types)| {
                let mut b = kafka_wire::messages::share_acknowledge_request::AcknowledgementBatch::default();
                b.first_offset = *first;
                b.last_offset = *last;
                b.acknowledge_types = types.clone();
                b
            })
            .collect();
        t.partitions.push(p);
        r.topics.push(t);
    }
    r
}

/// The acquired ranges of a fetch answer, by (partition, first, last, delivery count), and its partition errors.
pub fn acquired(r: &ShareFetchResponse) -> Vec<(i32, i64, i64, i16)> {
    r.responses
        .iter()
        .flat_map(|t| &t.partitions)
        .flat_map(|p| p.acquired_records.iter().map(move |a| (p.partition_index, a.first_offset, a.last_offset, a.delivery_count)))
        .collect()
}

impl Rig {
    pub async fn share_heartbeat(&self, req: ShareGroupHeartbeatRequest) -> ShareGroupHeartbeatResponse {
        let Response::ShareGroupHeartbeat(r) = self.ask(1, Request::ShareGroupHeartbeat(req)).await else {
            panic!("not ShareGroupHeartbeat")
        };
        r
    }

    /// Joins `member` to `group` reading `topics`; its epoch.
    pub async fn share_join(&self, group: &str, member: &str, topics: &[&str]) -> ShareGroupHeartbeatResponse {
        self.share_heartbeat(heartbeat_request(group, member, 0, Some(topics))).await
    }

    pub async fn share_fetch_at(&self, version: i16, req: ShareFetchRequest) -> ShareFetchResponse {
        let Response::ShareFetch(r) = self.ask(version, Request::ShareFetch(req)).await else {
            panic!("not ShareFetch")
        };
        r
    }

    pub async fn share_fetch(&self, req: ShareFetchRequest) -> ShareFetchResponse {
        self.share_fetch_at(1, req).await
    }

    pub async fn share_ack_at(&self, version: i16, req: ShareAcknowledgeRequest) -> ShareAcknowledgeResponse {
        let Response::ShareAcknowledge(r) = self.ask(version, Request::ShareAcknowledge(req)).await else {
            panic!("not ShareAcknowledge")
        };
        r
    }

    pub async fn share_describe(&self, groups: &[&str]) -> ShareGroupDescribeResponse {
        let mut req = ShareGroupDescribeRequest::default();
        req.group_ids = groups.iter().map(|g| GroupId(StrBytes::from_string((*g).to_owned()))).collect();
        let Response::ShareGroupDescribe(r) = self.ask(1, Request::ShareGroupDescribe(req)).await else {
            panic!("not ShareGroupDescribe")
        };
        r
    }

    /// DescribeShareGroupOffsets of one group: every partition it has (`topics` None) or those named.
    pub async fn share_offsets(
        &self,
        version: i16,
        group: &str,
        topics: Option<&[(&str, &[i32])]>,
    ) -> DescribeShareGroupOffsetsResponse {
        let mut g = DescribeShareGroupOffsetsRequestGroup::default();
        g.group_id = GroupId(StrBytes::from_string(group.to_owned()));
        g.topics = topics.map(|ts| {
            ts.iter()
                .map(|(name, ps)| {
                    let mut t = DescribeShareGroupOffsetsRequestTopic::default();
                    t.topic_name = wire_name(name);
                    t.partitions = ps.to_vec();
                    t
                })
                .collect()
        });
        let mut req = DescribeShareGroupOffsetsRequest::default();
        req.groups.push(g);
        let Response::DescribeShareGroupOffsets(r) = self.ask(version, Request::DescribeShareGroupOffsets(req)).await else {
            panic!("not DescribeShareGroupOffsets")
        };
        r
    }

    /// (topic, partition, start offset, lag) of every partition DescribeShareGroupOffsets v1 tells of `group`.
    pub async fn share_starts(&self, group: &str) -> Vec<(String, i32, i64, i64)> {
        let r = self.share_offsets(1, group, None).await;
        r.groups[0]
            .topics
            .iter()
            .flat_map(|t| t.partitions.iter().map(move |p| (t.topic_name.0.to_string(), p.partition_index, p.start_offset, p.lag)))
            .collect()
    }

    pub async fn share_alter(&self, group: &str, topic: &str, starts: &[(i32, i64)]) -> AlterShareGroupOffsetsResponse {
        let mut t = AlterShareGroupOffsetsRequestTopic::default();
        t.topic_name = wire_name(topic);
        t.partitions = starts
            .iter()
            .map(|(p, s)| {
                let mut x = AlterShareGroupOffsetsRequestPartition::default();
                x.partition_index = *p;
                x.start_offset = *s;
                x
            })
            .collect();
        let mut req = AlterShareGroupOffsetsRequest::default();
        req.group_id = GroupId(StrBytes::from_string(group.to_owned()));
        req.topics.push(t);
        let Response::AlterShareGroupOffsets(r) = self.ask(0, Request::AlterShareGroupOffsets(req)).await else {
            panic!("not AlterShareGroupOffsets")
        };
        r
    }

    pub async fn share_delete_offsets(&self, group: &str, topics: &[&str]) -> DeleteShareGroupOffsetsResponse {
        let mut req = DeleteShareGroupOffsetsRequest::default();
        req.group_id = GroupId(StrBytes::from_string(group.to_owned()));
        req.topics = topics
            .iter()
            .map(|t| {
                let mut x = DeleteShareGroupOffsetsRequestTopic::default();
                x.topic_name = wire_name(t);
                x
            })
            .collect();
        let Response::DeleteShareGroupOffsets(r) = self.ask(0, Request::DeleteShareGroupOffsets(req)).await else {
            panic!("not DeleteShareGroupOffsets")
        };
        r
    }

    /// Sets group configs; the error code of the resource.
    pub async fn group_config(&self, group: &str, configs: &[(&str, &str)]) -> i16 {
        let mut res = AlterConfigsResource::default();
        res.resource_type = 32;
        res.resource_name = StrBytes::from_string(group.to_owned());
        res.configs = configs
            .iter()
            .map(|(k, v)| {
                let mut c = AlterableConfig::default();
                c.name = StrBytes::from_string((*k).to_owned());
                c.config_operation = 0;
                c.value = Some(StrBytes::from_string((*v).to_owned()));
                c
            })
            .collect();
        let mut req = IncrementalAlterConfigsRequest::default();
        req.resources.push(res);
        let Response::IncrementalAlterConfigs(r) = self.ask(1, Request::IncrementalAlterConfigs(req)).await else {
            panic!("not IncrementalAlterConfigs")
        };
        r.responses[0].error_code
    }
}
