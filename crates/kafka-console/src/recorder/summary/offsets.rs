//! ListOffsets and the group offset APIs: topics and partitions, the group, and each partition's result.

use kafka_wire::messages::{
    ListOffsetsRequest, ListOffsetsResponse, OffsetCommitRequest, OffsetCommitResponse,
    OffsetDeleteRequest, OffsetDeleteResponse, OffsetFetchRequest, OffsetFetchResponse,
    OffsetForLeaderEpochRequest, OffsetForLeaderEpochResponse,
};

use super::produce::named;

use super::n;
use crate::recorder::facts::Facts;

pub(super) fn list_request(r: &ListOffsetsRequest, f: &mut Facts) {
    f.isolation = Some(r.isolation_level);
    for t in &r.topics {
        f.topic(&t.name.0, t.partitions.iter().map(|p| p.partition_index));
    }
}

pub(super) fn epoch_request(r: &OffsetForLeaderEpochRequest, f: &mut Facts) {
    for t in &r.topics {
        f.topic(&t.topic.0, t.partitions.iter().map(|p| p.partition));
    }
}

/// Each partition's code and, where it was found, the end offset of the epoch asked about.
pub(super) fn epoch_response(r: &OffsetForLeaderEpochResponse, f: &mut Facts) {
    for t in &r.topics {
        for p in &t.partitions {
            let found = p.error_code == 0 && p.end_offset >= 0;
            f.result(
                &t.topic.0,
                p.partition,
                p.error_code,
                found.then_some(p.end_offset),
            );
        }
    }
}

pub(super) fn list_response(r: &ListOffsetsResponse, f: &mut Facts) {
    for t in &r.topics {
        for p in &t.partitions {
            f.result(
                &t.name.0,
                p.partition_index,
                p.error_code,
                (p.error_code == 0).then_some(p.offset),
            );
        }
    }
}

pub(super) fn commit_request(r: &OffsetCommitRequest, f: &mut Facts) {
    f.group(&r.group_id.0);
    f.member(&r.member_id);
    f.generation = Some(r.generation_id_or_member_epoch);
    // (v10 names topics by id, KIP-848: the summary shows the id, as Fetch's does.)
    for t in &r.topics {
        f.topic(
            &named(&t.name.0, t.topic_id),
            t.partitions.iter().map(|p| p.partition_index),
        );
    }
}

pub(super) fn commit_response(r: &OffsetCommitResponse, f: &mut Facts) {
    for t in &r.topics {
        for p in &t.partitions {
            f.result(
                &named(&t.name.0, t.topic_id),
                p.partition_index,
                p.error_code,
                None,
            );
        }
    }
}

pub(super) fn fetch_request(r: &OffsetFetchRequest, f: &mut Facts) {
    f.group(&r.group_id.0);
    for g in &r.groups {
        f.group(&g.group_id.0);
        for t in g.topics.iter().flatten() {
            f.topic(
                &named(&t.name.0, t.topic_id),
                t.partition_indexes.iter().copied(),
            );
        }
    }
    for t in r.topics.iter().flatten() {
        f.topic(&t.name.0, t.partition_indexes.iter().copied());
    }
    if r.topics.is_none() && r.groups.is_empty() {
        f.about("every committed offset");
    }
    f.asked = n(r.groups.len().max(1));
}

pub(super) fn fetch_response(r: &OffsetFetchResponse, f: &mut Facts) {
    f.error(r.error_code);
    for t in &r.topics {
        for p in &t.partitions {
            f.result(
                &t.name.0,
                p.partition_index,
                p.error_code,
                (p.error_code == 0 && p.committed_offset >= 0).then_some(p.committed_offset),
            );
        }
    }
    for g in &r.groups {
        f.error(g.error_code);
        for t in &g.topics {
            for p in &t.partitions {
                f.result(
                    &named(&t.name.0, t.topic_id),
                    p.partition_index,
                    p.error_code,
                    (p.error_code == 0 && p.committed_offset >= 0).then_some(p.committed_offset),
                );
            }
        }
    }
}

pub(super) fn delete_request(r: &OffsetDeleteRequest, f: &mut Facts) {
    f.group(&r.group_id.0);
    for t in &r.topics {
        f.topic(&t.name.0, t.partitions.iter().map(|p| p.partition_index));
    }
}

pub(super) fn delete_response(r: &OffsetDeleteResponse, f: &mut Facts) {
    f.error(r.error_code);
    for t in &r.topics {
        for p in &t.partitions {
            f.result(&t.name.0, p.partition_index, p.error_code, None);
        }
    }
}
