//! Requests and readings for the admin APIs of debt K3 (`admin_apis.rs`, `admin_apis_rights.rs`).
#![allow(
    unreachable_pub,
    dead_code,
    unused_imports,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::type_complexity,
    reason = "test helpers, each used by some of the test targets that include them"
)]

use super::acls::{
    ALLOW, ALTER, CLUSTER, DESCRIBE, LITERAL, closed_world, creation, grant, on, user,
};
use super::{Rig, rig, topic, wire_name};
use kafka_broker::Caller;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::alter_partition_reassignments_request::{
    ReassignablePartition, ReassignableTopic,
};
use kafka_wire::messages::alter_replica_log_dirs_request::{
    AlterReplicaLogDir, AlterReplicaLogDirTopic,
};
use kafka_wire::messages::describe_topic_partitions_request::{Cursor, TopicRequest};
use kafka_wire::messages::elect_leaders_request::TopicPartitions;
use kafka_wire::messages::list_partition_reassignments_request::ListPartitionReassignmentsTopics;
use kafka_wire::messages::update_features_request::FeatureUpdateKey;
use kafka_wire::messages::{
    AlterPartitionReassignmentsRequest, AlterReplicaLogDirsRequest, BrokerId,
    DescribeTopicPartitionsRequest, DescribeTopicPartitionsResponse, ElectLeadersRequest,
    ListConfigResourcesRequest, ListPartitionReassignmentsRequest, UpdateFeaturesRequest,
};
use kafka_wire::{ResponseError, StrBytes};

pub const DESCRIBE_CONFIGS: i8 = 10;

/// Topics made by the super user of a closed world.
pub async fn create_as_admin(
    rig: &Rig,
    topics: Vec<kafka_wire::messages::create_topics_request::CreatableTopic>,
) {
    let mut req = kafka_wire::messages::CreateTopicsRequest::default();
    req.topics = topics;
    let Response::CreateTopics(r) = rig
        .ask_as(&user("admin"), 7, Request::CreateTopics(req))
        .await
    else {
        panic!()
    };
    assert!(r.topics.iter().all(|t| t.error_code == 0));
}

pub async fn fixture() -> Rig {
    let rig = rig();
    rig.create(
        vec![topic("one", 1), topic("three", 3), topic("many", 5)],
        false,
    )
    .await;
    rig
}

pub fn dtp(
    names: &[&str],
    limit: i32,
    cursor: Option<(&str, i32)>,
) -> DescribeTopicPartitionsRequest {
    let mut req = DescribeTopicPartitionsRequest::default();
    req.response_partition_limit = limit;
    req.topics = (names.iter())
        .map(|n| {
            let mut t = TopicRequest::default();
            t.name = wire_name(n);
            t
        })
        .collect();
    req.cursor = cursor.map(|(name, p)| {
        let mut c = Cursor::default();
        c.topic_name = wire_name(name);
        c.partition_index = p;
        c
    });
    req
}

pub async fn describe(
    rig: &Rig,
    who: &Caller,
    req: DescribeTopicPartitionsRequest,
) -> DescribeTopicPartitionsResponse {
    let Response::DescribeTopicPartitions(r) = rig
        .ask_as(who, 0, Request::DescribeTopicPartitions(req))
        .await
    else {
        panic!("not DescribeTopicPartitions")
    };
    r
}

/// Each topic as (name, error, partitions listed), and the cursor.
pub fn pages(
    r: &DescribeTopicPartitionsResponse,
) -> (Vec<(String, i16, Vec<i32>)>, Option<(String, i32)>) {
    let topics = (r.topics.iter())
        .map(|t| {
            let name = t.name.as_ref().map_or(String::new(), |n| n.0.to_string());
            (
                name,
                t.error_code,
                t.partitions.iter().map(|p| p.partition_index).collect(),
            )
        })
        .collect();
    let cursor = r
        .next_cursor
        .as_ref()
        .map(|c| (c.topic_name.0.to_string(), c.partition_index));
    (topics, cursor)
}

pub fn own(v: &[(&str, i16, &[i32])]) -> Vec<(String, i16, Vec<i32>)> {
    v.iter()
        .map(|(n, e, p)| ((*n).to_owned(), *e, p.to_vec()))
        .collect()
}

pub fn elect(kind: i8, asks: Option<&[(&str, &[i32])]>) -> ElectLeadersRequest {
    let mut req = ElectLeadersRequest::default();
    req.election_type = kind;
    req.topic_partitions = asks.map(|asks| {
        asks.iter()
            .map(|(name, parts)| {
                let mut tp = TopicPartitions::default();
                tp.topic = wire_name(name);
                tp.partitions = parts.to_vec();
                tp
            })
            .collect()
    });
    req
}

pub async fn elect_codes(
    rig: &Rig,
    who: &Caller,
    version: i16,
    req: ElectLeadersRequest,
) -> (i16, Vec<(String, i32, i16, Option<String>)>) {
    let Response::ElectLeaders(r) = rig.ask_as(who, version, Request::ElectLeaders(req)).await
    else {
        panic!("not ElectLeaders")
    };
    let lines = (r.replica_election_results.iter())
        .flat_map(|t| {
            t.partition_result.iter().map(|p| {
                (
                    t.topic.0.to_string(),
                    p.partition_id,
                    p.error_code,
                    p.error_message.as_ref().map(ToString::to_string),
                )
            })
        })
        .collect();
    (r.error_code, lines)
}

pub fn reassign(asks: &[(&str, i32, Option<&[i32]>)]) -> AlterPartitionReassignmentsRequest {
    let mut req = AlterPartitionReassignmentsRequest::default();
    for (name, part, replicas) in asks {
        let mut t = ReassignableTopic::default();
        t.name = wire_name(name);
        let mut p = ReassignablePartition::default();
        p.partition_index = *part;
        p.replicas = replicas.map(|r| r.iter().map(|&b| BrokerId(b)).collect());
        t.partitions.push(p);
        req.topics.push(t);
    }
    req
}

pub fn move_logs(asks: &[(&str, &str, &[i32])]) -> AlterReplicaLogDirsRequest {
    let mut req = AlterReplicaLogDirsRequest::default();
    for (dir, name, parts) in asks {
        let mut d = AlterReplicaLogDir::default();
        d.path = StrBytes::from_string((*dir).to_owned());
        let mut t = AlterReplicaLogDirTopic::default();
        t.name = wire_name(name);
        t.partitions = parts.to_vec();
        d.topics.push(t);
        req.dirs.push(d);
    }
    req
}

pub fn update(asks: &[(&str, i16, i8)], validate_only: bool) -> UpdateFeaturesRequest {
    let mut req = UpdateFeaturesRequest::default();
    req.validate_only = validate_only;
    for (name, level, kind) in asks {
        let mut u = FeatureUpdateKey::default();
        u.feature = StrBytes::from_string((*name).to_owned());
        u.max_version_level = *level;
        u.upgrade_type = *kind;
        req.feature_updates.push(u);
    }
    req
}

pub async fn features(
    rig: &Rig,
    who: &Caller,
    version: i16,
    req: UpdateFeaturesRequest,
) -> (i16, Option<String>, usize) {
    let Response::UpdateFeatures(r) = rig.ask_as(who, version, Request::UpdateFeatures(req)).await
    else {
        panic!()
    };
    (
        r.error_code,
        r.error_message.map(|m| m.to_string()),
        r.results.len(),
    )
}
