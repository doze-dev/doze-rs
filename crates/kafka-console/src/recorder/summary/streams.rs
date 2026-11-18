//! The streams group APIs (KIP-1071): the group, the member and its epoch, the topology's topics on a join, how many tasks the
//! member was given, and the statuses the broker returned.

use kafka_wire::messages::streams_group_heartbeat_response::TaskIds;
use kafka_wire::messages::{
    StreamsGroupDescribeRequest, StreamsGroupDescribeResponse, StreamsGroupHeartbeatRequest,
    StreamsGroupHeartbeatResponse, StreamsGroupTopologyDescriptionUpdateRequest,
    StreamsGroupTopologyDescriptionUpdateResponse,
};

use super::n;
use crate::recorder::facts::Facts;

pub(super) fn heartbeat_request(r: &StreamsGroupHeartbeatRequest, f: &mut Facts) {
    f.group(&r.group_id.0);
    f.member(&r.member_id);
    f.generation = Some(r.member_epoch);
    for s in r.topology.iter().flat_map(|t| &t.subtopologies) {
        for t in &s.source_topics {
            f.topic(&t.0, []);
        }
    }
}

pub(super) fn heartbeat_response(r: &StreamsGroupHeartbeatResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.say(r.error_message.as_deref());
    f.generation = Some(r.member_epoch);
    if r.active_tasks.is_some() {
        f.items = n(tasks(r.active_tasks.as_ref())
            + tasks(r.standby_tasks.as_ref())
            + tasks(r.warmup_tasks.as_ref()));
    }
    if let Some(first) = r.status.iter().flatten().next() {
        f.say(Some(&first.status_detail));
    }
}

fn tasks(list: Option<&Vec<TaskIds>>) -> usize {
    list.iter()
        .flat_map(|l| l.iter())
        .map(|t| t.partitions.len())
        .sum()
}

pub(super) fn describe_request(r: &StreamsGroupDescribeRequest, f: &mut Facts) {
    for g in &r.group_ids {
        f.group(&g.0);
    }
    f.asked = n(r.group_ids.len());
}

pub(super) fn describe_response(r: &StreamsGroupDescribeResponse, f: &mut Facts) {
    for g in &r.groups {
        f.error(g.error_code);
        f.say(g.error_message.as_deref());
    }
    f.items = n(r.groups.len());
}

pub(super) fn topology_update_request(
    r: &StreamsGroupTopologyDescriptionUpdateRequest,
    f: &mut Facts,
) {
    f.group(&r.group_id.0);
    f.member(&r.member_id);
}

pub(super) fn topology_update_response(
    r: &StreamsGroupTopologyDescriptionUpdateResponse,
    f: &mut Facts,
) {
    f.error(r.error_code);
    f.say(r.error_message.as_deref());
}
