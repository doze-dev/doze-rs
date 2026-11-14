//! The classic group APIs and the KIP-848 pair: the group, the member, the generation, the client's own reason.

use kafka_wire::messages::{
    ConsumerGroupDescribeRequest, ConsumerGroupDescribeResponse, ConsumerGroupHeartbeatRequest,
    ConsumerGroupHeartbeatResponse, DeleteGroupsRequest, DeleteGroupsResponse,
    DescribeGroupsRequest, DescribeGroupsResponse, FindCoordinatorRequest, FindCoordinatorResponse,
    HeartbeatRequest, HeartbeatResponse, JoinGroupRequest, JoinGroupResponse, LeaveGroupRequest,
    LeaveGroupResponse, ListGroupsRequest, ListGroupsResponse, SyncGroupRequest, SyncGroupResponse,
};

use super::n;
use crate::recorder::clip;
use crate::recorder::facts::Facts;

pub(super) fn find_request(r: &FindCoordinatorRequest, f: &mut Facts) {
    let what = if r.key_type == 1 {
        "transaction"
    } else {
        "group"
    };
    for key in std::iter::once(&r.key).chain(r.coordinator_keys.iter()) {
        if what == "group" {
            f.group(key);
        } else {
            f.txn(key);
        }
    }
    f.asked = n(r.coordinator_keys.len().max(1));
}

pub(super) fn find_response(r: &FindCoordinatorResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.say(r.error_message.as_deref());
    for c in &r.coordinators {
        f.error(c.error_code);
    }
}

pub(super) fn join_request(r: &JoinGroupRequest, f: &mut Facts) {
    f.group(&r.group_id.0);
    f.member(&r.member_id);
    f.reason = r.reason.as_deref().and_then(clip::text);
}

pub(super) fn join_response(r: &JoinGroupResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.generation = Some(r.generation_id);
    f.member(&r.member_id);
    f.items = n(r.members.len());
}

pub(super) fn heartbeat_request(r: &HeartbeatRequest, f: &mut Facts) {
    f.group(&r.group_id.0);
    f.member(&r.member_id);
    f.generation = Some(r.generation_id);
}

pub(super) fn heartbeat_response(r: &HeartbeatResponse, f: &mut Facts) {
    f.error(r.error_code);
}

pub(super) fn leave_request(r: &LeaveGroupRequest, f: &mut Facts) {
    f.group(&r.group_id.0);
    f.member(&r.member_id);
    for m in &r.members {
        f.member(&m.member_id);
        if f.reason.is_none() {
            f.reason = m.reason.as_deref().and_then(clip::text);
        }
    }
    f.asked = n(r.members.len().max(1));
}

pub(super) fn leave_response(r: &LeaveGroupResponse, f: &mut Facts) {
    f.error(r.error_code);
    for m in &r.members {
        f.error(m.error_code);
    }
}

pub(super) fn sync_request(r: &SyncGroupRequest, f: &mut Facts) {
    f.group(&r.group_id.0);
    f.member(&r.member_id);
    f.generation = Some(r.generation_id);
    // Whether this member is the one handing out the assignment: the request carries one only for the leader.
    f.items = n(r.assignments.len());
}

pub(super) fn sync_response(r: &SyncGroupResponse, f: &mut Facts) {
    f.error(r.error_code);
}

pub(super) fn describe_request(r: &DescribeGroupsRequest, f: &mut Facts) {
    for g in &r.groups {
        f.group(&g.0);
    }
    f.asked = n(r.groups.len());
}

pub(super) fn describe_response(r: &DescribeGroupsResponse, f: &mut Facts) {
    for g in &r.groups {
        f.error(g.error_code);
        f.say(g.error_message.as_deref());
    }
    f.items = n(r.groups.len());
}

pub(super) fn list_request(r: &ListGroupsRequest, f: &mut Facts) {
    f.asked = n(r.states_filter.len());
    f.about("every group");
}

pub(super) fn list_response(r: &ListGroupsResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.items = n(r.groups.len());
}

pub(super) fn delete_request(r: &DeleteGroupsRequest, f: &mut Facts) {
    for g in &r.groups_names {
        f.group(&g.0);
    }
    f.asked = n(r.groups_names.len());
}

pub(super) fn delete_response(r: &DeleteGroupsResponse, f: &mut Facts) {
    for g in &r.results {
        f.error(g.error_code);
    }
    f.items = n(r.results.len());
}

pub(super) fn consumer_heartbeat_request(r: &ConsumerGroupHeartbeatRequest, f: &mut Facts) {
    f.group(&r.group_id.0);
    f.member(&r.member_id);
    f.generation = Some(r.member_epoch);
    for t in r.subscribed_topic_names.iter().flatten() {
        f.topic(&t.0, []);
    }
}

pub(super) fn consumer_heartbeat_response(r: &ConsumerGroupHeartbeatResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.say(r.error_message.as_deref());
    if r.error_code == 0 {
        f.generation = Some(r.member_epoch);
    }
    f.items = r
        .assignment
        .as_ref()
        .map_or(0, |a| n(a.topic_partitions.len()));
}

pub(super) fn consumer_describe_request(r: &ConsumerGroupDescribeRequest, f: &mut Facts) {
    for g in &r.group_ids {
        f.group(&g.0);
    }
    f.asked = n(r.group_ids.len());
}

pub(super) fn consumer_describe_response(r: &ConsumerGroupDescribeResponse, f: &mut Facts) {
    for g in &r.groups {
        f.error(g.error_code);
        f.say(g.error_message.as_deref());
    }
    f.items = n(r.groups.len());
}
