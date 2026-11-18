//! The share group APIs (KIP-932): the group, the member, its epoch or session epoch, the records a fetch acquired (their
//! count and size, from the response's ranges and batch headers) and how many acknowledgement batches a request carried.

use kafka_wire::messages::{
    AlterShareGroupOffsetsRequest, AlterShareGroupOffsetsResponse, DeleteShareGroupOffsetsRequest,
    DeleteShareGroupOffsetsResponse, DescribeShareGroupOffsetsRequest,
    DescribeShareGroupOffsetsResponse, ShareAcknowledgeRequest, ShareAcknowledgeResponse,
    ShareFetchRequest, ShareFetchResponse, ShareGroupDescribeRequest, ShareGroupDescribeResponse,
    ShareGroupHeartbeatRequest, ShareGroupHeartbeatResponse,
};

use super::n;
use crate::recorder::facts::{Facts, batches};

pub(super) fn heartbeat_request(r: &ShareGroupHeartbeatRequest, f: &mut Facts) {
    f.group(&r.group_id.0);
    f.member(&r.member_id);
    f.generation = Some(r.member_epoch);
    for t in r.subscribed_topic_names.iter().flatten() {
        f.topic(&t.0, []);
    }
}

pub(super) fn heartbeat_response(r: &ShareGroupHeartbeatResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.say(r.error_message.as_deref());
    f.generation = Some(r.member_epoch);
    if let Some(a) = &r.assignment {
        f.items = n(a.topic_partitions.iter().map(|t| t.partitions.len()).sum());
    }
}

pub(super) fn describe_request(r: &ShareGroupDescribeRequest, f: &mut Facts) {
    for g in &r.group_ids {
        f.group(&g.0);
    }
    f.asked = n(r.group_ids.len());
}

pub(super) fn describe_response(r: &ShareGroupDescribeResponse, f: &mut Facts) {
    for g in &r.groups {
        f.error(g.error_code);
        f.say(g.error_message.as_deref());
    }
    f.items = n(r.groups.len());
}

pub(super) fn fetch_request(r: &ShareFetchRequest, f: &mut Facts) {
    if let Some(g) = &r.group_id {
        f.group(&g.0);
    }
    if let Some(m) = &r.member_id {
        f.member(m);
    }
    f.generation = Some(r.share_session_epoch);
    f.wait_ms = Some(r.max_wait_ms);
    f.asked = n(r.topics.iter().map(|t| t.partitions.len()).sum());
}

pub(super) fn fetch_response(r: &ShareFetchResponse, f: &mut Facts) {
    f.error(r.error_code);
    for t in &r.responses {
        for p in &t.partitions {
            f.error(p.error_code);
            f.error(p.acknowledge_error_code);
            f.say(p.acknowledge_error_message.as_deref());
            if let Some(records) = &p.records {
                let s = batches(records);
                f.batches = f.batches.saturating_add(s.batches);
                f.bytes = f.bytes.saturating_add(s.bytes);
            }
            let held: i64 = p
                .acquired_records
                .iter()
                .map(|a| a.last_offset - a.first_offset + 1)
                .sum();
            f.records = f
                .records
                .saturating_add(u32::try_from(held).unwrap_or(u32::MAX));
        }
    }
}

pub(super) fn acknowledge_request(r: &ShareAcknowledgeRequest, f: &mut Facts) {
    if let Some(g) = &r.group_id {
        f.group(&g.0);
    }
    if let Some(m) = &r.member_id {
        f.member(m);
    }
    f.generation = Some(r.share_session_epoch);
    f.asked = n(r
        .topics
        .iter()
        .flat_map(|t| &t.partitions)
        .map(|p| p.acknowledgement_batches.len())
        .sum());
}

pub(super) fn acknowledge_response(r: &ShareAcknowledgeResponse, f: &mut Facts) {
    f.error(r.error_code);
    for t in &r.responses {
        for p in &t.partitions {
            f.error(p.error_code);
            f.say(p.error_message.as_deref());
        }
    }
}

pub(super) fn describe_offsets_request(r: &DescribeShareGroupOffsetsRequest, f: &mut Facts) {
    for g in &r.groups {
        f.group(&g.group_id.0);
        for t in g.topics.iter().flatten() {
            f.topic(&t.topic_name.0, t.partitions.iter().copied());
        }
    }
    f.asked = n(r.groups.len());
}

pub(super) fn describe_offsets_response(r: &DescribeShareGroupOffsetsResponse, f: &mut Facts) {
    for g in &r.groups {
        f.error(g.error_code);
        f.say(g.error_message.as_deref());
        for t in &g.topics {
            for p in &t.partitions {
                f.result(
                    &t.topic_name.0,
                    p.partition_index,
                    p.error_code,
                    Some(p.start_offset),
                );
            }
        }
    }
}

pub(super) fn alter_offsets_request(r: &AlterShareGroupOffsetsRequest, f: &mut Facts) {
    f.group(&r.group_id.0);
    for t in &r.topics {
        f.topic(
            &t.topic_name.0,
            t.partitions.iter().map(|p| p.partition_index),
        );
    }
}

pub(super) fn alter_offsets_response(r: &AlterShareGroupOffsetsResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.say(r.error_message.as_deref());
    for t in &r.responses {
        for p in &t.partitions {
            f.result(&t.topic_name.0, p.partition_index, p.error_code, None);
        }
    }
}

pub(super) fn delete_offsets_request(r: &DeleteShareGroupOffsetsRequest, f: &mut Facts) {
    f.group(&r.group_id.0);
    for t in &r.topics {
        f.topic(&t.topic_name.0, []);
    }
}

pub(super) fn delete_offsets_response(r: &DeleteShareGroupOffsetsResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.say(r.error_message.as_deref());
    for t in &r.responses {
        f.error(t.error_code);
    }
    f.items = n(r.responses.len());
}
