//! Requests for DescribeGroups, ListGroups, DeleteGroups and OffsetDelete.
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

impl Rig {
    pub async fn describe_groups(
        &self,
        names: &[&str],
    ) -> kafka_wire::messages::DescribeGroupsResponse {
        let mut req = kafka_wire::messages::DescribeGroupsRequest::default();
        req.groups = names.iter().map(|n| GroupId(text(n))).collect();
        match self.ask(5, Request::DescribeGroups(req)).await {
            Response::DescribeGroups(r) => r,
            other => panic!("not DescribeGroups: {other:?}"),
        }
    }

    pub async fn list_groups(
        &self,
        states: &[&str],
        types: &[&str],
    ) -> Vec<(String, String, String)> {
        let mut req = kafka_wire::messages::ListGroupsRequest::default();
        req.states_filter = states.iter().map(|s| text(s)).collect();
        req.types_filter = types.iter().map(|s| text(s)).collect();
        match self.ask(5, Request::ListGroups(req)).await {
            Response::ListGroups(r) => r
                .groups
                .iter()
                .map(|g| {
                    (
                        g.group_id.to_string(),
                        g.group_state.to_string(),
                        g.protocol_type.to_string(),
                    )
                })
                .collect(),
            other => panic!("not ListGroups: {other:?}"),
        }
    }

    /// (group, error code) in the order answered.
    pub async fn delete_groups(&self, names: &[&str]) -> Vec<(String, i16)> {
        let mut req = kafka_wire::messages::DeleteGroupsRequest::default();
        req.groups_names = names.iter().map(|n| GroupId(text(n))).collect();
        match self.ask(2, Request::DeleteGroups(req)).await {
            Response::DeleteGroups(r) => r
                .results
                .iter()
                .map(|x| (x.group_id.to_string(), x.error_code))
                .collect(),
            other => panic!("not DeleteGroups: {other:?}"),
        }
    }

    /// The top-level code and, per (topic, partition) asked, its code.
    pub async fn offset_delete(&self, group: &str, asked: &[(&str, i32)]) -> (i16, Vec<i16>) {
        use kafka_wire::messages::offset_delete_request::{
            OffsetDeleteRequestPartition, OffsetDeleteRequestTopic,
        };
        let mut req = kafka_wire::messages::OffsetDeleteRequest::default();
        req.group_id = GroupId(text(group));
        for (topic, partition) in asked {
            let mut p = OffsetDeleteRequestPartition::default();
            p.partition_index = *partition;
            match req.topics.iter_mut().find(|t| t.name.as_str() == *topic) {
                Some(t) => t.partitions.push(p),
                None => {
                    let mut t = OffsetDeleteRequestTopic::default();
                    t.name = kafka_wire::messages::TopicName(text(topic));
                    t.partitions.push(p);
                    req.topics.push(t);
                }
            }
        }
        match self.ask(0, Request::OffsetDelete(req)).await {
            Response::OffsetDelete(r) => (
                r.error_code,
                r.topics
                    .iter()
                    .flat_map(|t| t.partitions.iter().map(|p| p.error_code))
                    .collect(),
            ),
            other => panic!("not OffsetDelete: {other:?}"),
        }
    }
}

/// A consumer's subscription to `topics`, as a client sends it in a JoinGroup (version 0, empty user data).
pub fn subscription(topics: &[&str]) -> String {
    let mut out = vec![0, 0];
    out.extend_from_slice(&(topics.len() as i32).to_be_bytes());
    for t in topics {
        out.extend_from_slice(&(t.len() as i16).to_be_bytes());
        out.extend_from_slice(t.as_bytes());
    }
    out.extend_from_slice(&0_i32.to_be_bytes()); // empty user data
    out.iter().map(|b| char::from(*b)).collect()
}
