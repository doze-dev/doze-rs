//! Requests for the consumer-group (KIP-848) APIs.
#![allow(
    unreachable_pub,
    dead_code,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::Rig;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::consumer_group_describe_response::DescribedGroup;
use kafka_wire::messages::consumer_group_heartbeat_request::TopicPartitions;
use kafka_wire::messages::{
    ConsumerGroupDescribeRequest, ConsumerGroupHeartbeatRequest, ConsumerGroupHeartbeatResponse,
    GroupId, TopicName,
};

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

/// A heartbeat that joins `group` as `member`, subscribing to `topics`.
pub fn join_848(group: &str, member: &str, topics: &[&str]) -> ConsumerGroupHeartbeatRequest {
    let mut req = ConsumerGroupHeartbeatRequest::default();
    req.group_id = GroupId(text(group));
    req.member_id = text(member);
    req.member_epoch = 0;
    req.rebalance_timeout_ms = 60_000;
    req.subscribed_topic_names = Some(topics.iter().map(|t| TopicName(text(t))).collect());
    req.topic_partitions = Some(Vec::new());
    req
}

/// A heartbeat that keeps on at `epoch`, saying what the member owns (`None`: not saying).
pub fn beat_848(
    group: &str,
    member: &str,
    epoch: i32,
    owned: Option<&[(kafka_wire::Uuid, Vec<i32>)]>,
) -> ConsumerGroupHeartbeatRequest {
    let mut req = ConsumerGroupHeartbeatRequest::default();
    req.group_id = GroupId(text(group));
    req.member_id = text(member);
    req.member_epoch = epoch;
    req.topic_partitions = owned.map(|o| {
        o.iter()
            .map(|(id, ps)| {
                let mut tp = TopicPartitions::default();
                tp.topic_id = *id;
                tp.partitions = ps.clone();
                tp
            })
            .collect()
    });
    req
}

impl Rig {
    pub async fn heartbeat_848(
        &self,
        version: i16,
        req: ConsumerGroupHeartbeatRequest,
    ) -> ConsumerGroupHeartbeatResponse {
        let Response::ConsumerGroupHeartbeat(r) = self
            .ask(version, Request::ConsumerGroupHeartbeat(req))
            .await
        else {
            panic!("not ConsumerGroupHeartbeat")
        };
        r
    }

    pub async fn describe_848(&self, version: i16, groups: &[&str]) -> Vec<DescribedGroup> {
        let mut req = ConsumerGroupDescribeRequest::default();
        req.group_ids = groups.iter().map(|g| GroupId(text(g))).collect();
        let Response::ConsumerGroupDescribe(r) =
            self.ask(version, Request::ConsumerGroupDescribe(req)).await
        else {
            panic!("not ConsumerGroupDescribe")
        };
        r.groups
    }
}

/// The partitions of a heartbeat's answer, as (topic, partition) pairs.
pub fn assigned(r: &ConsumerGroupHeartbeatResponse) -> Option<Vec<(kafka_wire::Uuid, i32)>> {
    r.assignment.as_ref().map(|a| {
        a.topic_partitions
            .iter()
            .flat_map(|t| t.partitions.iter().map(|p| (t.topic_id, *p)))
            .collect()
    })
}

impl Rig {
    /// DescribeGroups' state for one group.
    pub async fn describe_groups_state(&self, group: &str) -> String {
        let mut req = kafka_wire::messages::DescribeGroupsRequest::default();
        req.groups = vec![GroupId(text(group))];
        let Response::DescribeGroups(r) = self.ask(5, Request::DescribeGroups(req)).await else {
            panic!("not DescribeGroups")
        };
        r.groups[0].group_state.as_str().to_owned()
    }

    /// Every group ListGroups names: (id, type, state).
    pub async fn list_groups_typed(&self) -> Vec<(String, String, String)> {
        let Response::ListGroups(r) = self
            .ask(
                5,
                Request::ListGroups(kafka_wire::messages::ListGroupsRequest::default()),
            )
            .await
        else {
            panic!("not ListGroups")
        };
        r.groups
            .iter()
            .map(|g| {
                (
                    g.group_id.0.as_str().to_owned(),
                    g.group_type.as_str().to_owned(),
                    g.group_state.as_str().to_owned(),
                )
            })
            .collect()
    }

    /// DeleteGroups for one group: its error code.
    pub async fn delete_group(&self, group: &str) -> i16 {
        let mut req = kafka_wire::messages::DeleteGroupsRequest::default();
        req.groups_names = vec![GroupId(text(group))];
        let Response::DeleteGroups(r) = self.ask(2, Request::DeleteGroups(req)).await else {
            panic!("not DeleteGroups")
        };
        r.results[0].error_code
    }

    /// OffsetDelete for (topic, partition) pairs of one group: each partition's error code.
    pub async fn delete_offsets(&self, group: &str, partitions: &[(&str, i32)]) -> Vec<i16> {
        use kafka_wire::messages::offset_delete_request::{
            OffsetDeleteRequestPartition, OffsetDeleteRequestTopic,
        };
        let mut req = kafka_wire::messages::OffsetDeleteRequest::default();
        req.group_id = GroupId(text(group));
        for (name, partition) in partitions {
            let mut t = OffsetDeleteRequestTopic::default();
            t.name = TopicName(text(name));
            let mut p = OffsetDeleteRequestPartition::default();
            p.partition_index = *partition;
            t.partitions.push(p);
            req.topics.push(t);
        }
        let Response::OffsetDelete(r) = self.ask(0, Request::OffsetDelete(req)).await else {
            panic!("not OffsetDelete")
        };
        r.topics
            .iter()
            .flat_map(|t| t.partitions.iter().map(|p| p.error_code))
            .collect()
    }
}
