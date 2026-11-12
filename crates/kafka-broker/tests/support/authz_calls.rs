//! The group APIs put to a broker as a given caller, for the authorization tests.
#![allow(
    unreachable_pub,
    dead_code,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::gx::{group_id, text, topic_name};
use crate::support::{Rig, join_request};
use kafka_broker::Caller;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::consumer_group_describe_response::DescribedGroup as DescribedConsumerGroup;
use kafka_wire::messages::leave_group_request::MemberIdentity;
use kafka_wire::messages::offset_commit_request::{
    OffsetCommitRequestPartition, OffsetCommitRequestTopic,
};
use kafka_wire::messages::offset_delete_request::{
    OffsetDeleteRequestPartition, OffsetDeleteRequestTopic,
};
use kafka_wire::messages::offset_fetch_request::{
    OffsetFetchRequestGroup, OffsetFetchRequestTopic, OffsetFetchRequestTopics,
};
use kafka_wire::messages::{
    ConsumerGroupDescribeRequest, ConsumerGroupHeartbeatRequest, ConsumerGroupHeartbeatResponse,
    DeleteGroupsRequest, DescribeGroupsRequest, DescribeGroupsResponse, FindCoordinatorRequest,
    FindCoordinatorResponse, HeartbeatRequest, HeartbeatResponse, JoinGroupResponse,
    LeaveGroupRequest, LeaveGroupResponse, ListGroupsRequest, OffsetCommitRequest,
    OffsetCommitResponse, OffsetDeleteRequest, OffsetDeleteResponse, OffsetFetchRequest,
    OffsetFetchResponse, SyncGroupRequest, SyncGroupResponse,
};

pub async fn find_coordinator(
    rig: &Rig,
    who: &Caller,
    version: i16,
    keys: &[&str],
    key_type: i8,
) -> FindCoordinatorResponse {
    let mut req = FindCoordinatorRequest::default();
    req.key_type = key_type;
    if version >= 4 {
        req.coordinator_keys = keys.iter().map(|k| text(k)).collect();
    } else {
        req.key = text(keys[0]);
    }
    match rig
        .ask_as(who, version, Request::FindCoordinator(req))
        .await
    {
        Response::FindCoordinator(r) => r,
        other => panic!("not FindCoordinator: {other:?}"),
    }
}

/// A JoinGroup of `member` (use version 3 or less to be taken in at once) to `group`.
pub async fn join(
    rig: &Rig,
    who: &Caller,
    version: i16,
    group: &str,
    member: &str,
) -> JoinGroupResponse {
    let req = join_request(group, member, None, &[("range", "")]);
    match rig.ask_as(who, version, Request::JoinGroup(req)).await {
        Response::JoinGroup(r) => r,
        other => panic!("not JoinGroup: {other:?}"),
    }
}

/// A SyncGroup of generation 1; from version 5 it names the protocol only if `protocol` says so.
pub async fn sync(
    rig: &Rig,
    who: &Caller,
    version: i16,
    group: &str,
    member: &str,
    protocol: bool,
) -> SyncGroupResponse {
    let mut req = SyncGroupRequest::default();
    req.group_id = group_id(group);
    req.member_id = text(member);
    req.generation_id = 1;
    if protocol {
        req.protocol_type = Some(text("consumer"));
        req.protocol_name = Some(text("range"));
    } else {
        req.protocol_type = None;
        req.protocol_name = None;
    }
    match rig.ask_as(who, version, Request::SyncGroup(req)).await {
        Response::SyncGroup(r) => r,
        other => panic!("not SyncGroup: {other:?}"),
    }
}

pub async fn heartbeat(rig: &Rig, who: &Caller, group: &str, member: &str) -> HeartbeatResponse {
    let mut req = HeartbeatRequest::default();
    req.group_id = group_id(group);
    req.member_id = text(member);
    req.generation_id = 1;
    match rig.ask_as(who, 4, Request::Heartbeat(req)).await {
        Response::Heartbeat(r) => r,
        other => panic!("not Heartbeat: {other:?}"),
    }
}

pub async fn leave(
    rig: &Rig,
    who: &Caller,
    version: i16,
    group: &str,
    member: &str,
) -> LeaveGroupResponse {
    let mut req = LeaveGroupRequest::default();
    req.group_id = group_id(group);
    if version < 3 {
        req.member_id = text(member);
    } else {
        let mut m = MemberIdentity::default();
        m.member_id = text(member);
        req.members = vec![m];
    }
    match rig.ask_as(who, version, Request::LeaveGroup(req)).await {
        Response::LeaveGroup(r) => r,
        other => panic!("not LeaveGroup: {other:?}"),
    }
}

/// An OffsetCommit by no member (a simple consumer): `(topic, partition, offset)` for each partition.
pub async fn commit(
    rig: &Rig,
    who: &Caller,
    version: i16,
    group: &str,
    commits: &[(&str, i32, i64)],
) -> OffsetCommitResponse {
    let mut req = OffsetCommitRequest::default();
    req.group_id = group_id(group);
    req.generation_id_or_member_epoch = -1;
    for (name, partition, offset) in commits {
        let mut p = OffsetCommitRequestPartition::default();
        p.partition_index = *partition;
        p.committed_offset = *offset;
        match req.topics.iter_mut().find(|t| t.name.as_str() == *name) {
            Some(t) => t.partitions.push(p),
            None => {
                let mut t = OffsetCommitRequestTopic::default();
                t.name = topic_name(name);
                t.partitions.push(p);
                req.topics.push(t);
            }
        }
    }
    match rig.ask_as(who, version, Request::OffsetCommit(req)).await {
        Response::OffsetCommit(r) => r,
        other => panic!("not OffsetCommit: {other:?}"),
    }
}

/// The code of the first partition of an OffsetCommit.
pub fn first_commit_code(r: &OffsetCommitResponse) -> i16 {
    r.topics[0].partitions[0].error_code
}

/// An OffsetFetch of `group` for `asked` (`None`: every offset). Versions 8 and later batch the groups.
pub async fn fetch(
    rig: &Rig,
    who: &Caller,
    version: i16,
    group: &str,
    asked: Option<&[(&str, &[i32])]>,
) -> OffsetFetchResponse {
    let mut req = OffsetFetchRequest::default();
    if version >= 8 {
        let mut g = OffsetFetchRequestGroup::default();
        g.group_id = group_id(group);
        g.topics = asked.map(|ts| {
            ts.iter()
                .map(|(n, ps)| {
                    let mut t = OffsetFetchRequestTopics::default();
                    t.name = topic_name(n);
                    t.partition_indexes = ps.to_vec();
                    t
                })
                .collect()
        });
        req.groups.push(g);
    } else {
        req.group_id = group_id(group);
        req.topics = asked.map(|ts| {
            ts.iter()
                .map(|(n, ps)| {
                    let mut t = OffsetFetchRequestTopic::default();
                    t.name = topic_name(n);
                    t.partition_indexes = ps.to_vec();
                    t
                })
                .collect()
        });
    }
    match rig.ask_as(who, version, Request::OffsetFetch(req)).await {
        Response::OffsetFetch(r) => r,
        other => panic!("not OffsetFetch: {other:?}"),
    }
}

/// The group-level code of an OffsetFetch at version 8, or the top-level one before.
pub fn fetch_code(version: i16, r: &OffsetFetchResponse) -> i16 {
    if version >= 8 {
        r.groups[0].error_code
    } else {
        r.error_code
    }
}

pub async fn describe_groups(
    rig: &Rig,
    who: &Caller,
    version: i16,
    names: &[&str],
    operations: bool,
) -> DescribeGroupsResponse {
    let mut req = DescribeGroupsRequest::default();
    req.groups = names.iter().map(|n| group_id(n)).collect();
    req.include_authorized_operations = operations;
    match rig.ask_as(who, version, Request::DescribeGroups(req)).await {
        Response::DescribeGroups(r) => r,
        other => panic!("not DescribeGroups: {other:?}"),
    }
}

/// The ids of the groups ListGroups shows, in the order shown.
pub async fn list_groups(rig: &Rig, who: &Caller) -> Vec<String> {
    match rig
        .ask_as(who, 5, Request::ListGroups(ListGroupsRequest::default()))
        .await
    {
        Response::ListGroups(r) => {
            assert_eq!(r.error_code, 0);
            r.groups.iter().map(|g| g.group_id.to_string()).collect()
        }
        other => panic!("not ListGroups: {other:?}"),
    }
}

/// `(group, code)` in the order answered.
pub async fn delete_groups(rig: &Rig, who: &Caller, names: &[&str]) -> Vec<(String, i16)> {
    let mut req = DeleteGroupsRequest::default();
    req.groups_names = names.iter().map(|n| group_id(n)).collect();
    match rig.ask_as(who, 2, Request::DeleteGroups(req)).await {
        Response::DeleteGroups(r) => r
            .results
            .iter()
            .map(|x| (x.group_id.to_string(), x.error_code))
            .collect(),
        other => panic!("not DeleteGroups: {other:?}"),
    }
}

pub async fn offset_delete(
    rig: &Rig,
    who: &Caller,
    group: &str,
    asked: &[(&str, i32)],
) -> OffsetDeleteResponse {
    let mut req = OffsetDeleteRequest::default();
    req.group_id = group_id(group);
    for (topic, partition) in asked {
        let mut p = OffsetDeleteRequestPartition::default();
        p.partition_index = *partition;
        match req.topics.iter_mut().find(|t| t.name.as_str() == *topic) {
            Some(t) => t.partitions.push(p),
            None => {
                let mut t = OffsetDeleteRequestTopic::default();
                t.name = topic_name(topic);
                t.partitions.push(p);
                req.topics.push(t);
            }
        }
    }
    match rig.ask_as(who, 0, Request::OffsetDelete(req)).await {
        Response::OffsetDelete(r) => r,
        other => panic!("not OffsetDelete: {other:?}"),
    }
}

/// The codes of an OffsetDelete: the top-level one, then each partition's.
pub fn delete_codes(r: &OffsetDeleteResponse) -> (i16, Vec<i16>) {
    (
        r.error_code,
        r.topics
            .iter()
            .flat_map(|t| t.partitions.iter().map(|p| p.error_code))
            .collect(),
    )
}

pub async fn heartbeat_848(
    rig: &Rig,
    who: &Caller,
    version: i16,
    req: ConsumerGroupHeartbeatRequest,
) -> ConsumerGroupHeartbeatResponse {
    match rig
        .ask_as(who, version, Request::ConsumerGroupHeartbeat(req))
        .await
    {
        Response::ConsumerGroupHeartbeat(r) => r,
        other => panic!("not ConsumerGroupHeartbeat: {other:?}"),
    }
}

pub async fn describe_848(
    rig: &Rig,
    who: &Caller,
    version: i16,
    names: &[&str],
    operations: bool,
) -> Vec<DescribedConsumerGroup> {
    let mut req = ConsumerGroupDescribeRequest::default();
    req.group_ids = names.iter().map(|n| group_id(n)).collect();
    req.include_authorized_operations = operations;
    match rig
        .ask_as(who, version, Request::ConsumerGroupDescribe(req))
        .await
    {
        Response::ConsumerGroupDescribe(r) => r.groups,
        other => panic!("not ConsumerGroupDescribe: {other:?}"),
    }
}
