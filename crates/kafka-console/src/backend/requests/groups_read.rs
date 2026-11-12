//! The requests of the Groups pages: describe a group (either protocol), what it committed, the admin commit of a reset,
//! the deletion of offsets and of the group. Each is bounded; none is sent by a GET except the reads.

use kafka_wire::StrBytes;
use kafka_wire::api::Request;
use kafka_wire::messages::offset_commit_request::{
    OffsetCommitRequestPartition, OffsetCommitRequestTopic,
};
use kafka_wire::messages::offset_delete_request::{
    OffsetDeleteRequestPartition, OffsetDeleteRequestTopic,
};
use kafka_wire::messages::offset_fetch_request::OffsetFetchRequestGroup;
use kafka_wire::messages::{
    ConsumerGroupDescribeRequest, DeleteGroupsRequest, DescribeGroupsRequest, GroupId,
    OffsetCommitRequest, OffsetDeleteRequest, OffsetFetchRequest, TopicName,
};

/// The most groups one describe names.
pub const MOST_DESCRIBED: usize = 100;

fn ids(names: &[String]) -> Vec<GroupId> {
    names
        .iter()
        .take(MOST_DESCRIBED)
        .map(|g| GroupId(StrBytes::from_string(g.clone())))
        .collect()
}

/// DescribeGroups for these groups (the classic protocol; a consumer group of the new one answers Dead).
#[must_use]
pub fn describe_groups(names: &[String]) -> Request {
    let mut r = DescribeGroupsRequest::default();
    r.groups = ids(names);
    r.include_authorized_operations = false;
    Request::DescribeGroups(r)
}

/// ConsumerGroupDescribe for these groups (KIP-848).
#[must_use]
pub fn consumer_group_describe(names: &[String]) -> Request {
    let mut r = ConsumerGroupDescribeRequest::default();
    r.group_ids = ids(names);
    r.include_authorized_operations = false;
    Request::ConsumerGroupDescribe(r)
}

/// Everything one group committed, on every topic and partition, waiting for a pending transaction's offsets to settle
/// (`require_stable`), as a consumer that must not read a half-committed state does.
#[must_use]
pub fn offsets_of(group: &str) -> Request {
    let mut r = OffsetFetchRequest::default();
    r.require_stable = true;
    let mut g = OffsetFetchRequestGroup::default();
    g.group_id = GroupId(StrBytes::from_string(group.to_owned()));
    g.topics = None;
    r.groups = vec![g];
    Request::OffsetFetch(r)
}

/// A commit by an administrator: no generation, no member (`kafka-consumer-groups --reset-offsets` does the same). The broker
/// accepts it only while the group has no members. At most 4,096 partitions.
#[must_use]
pub fn admin_commit(group: &str, topic: &str, offsets: &[(i32, i64)]) -> Request {
    let mut t = OffsetCommitRequestTopic::default();
    t.name = TopicName(StrBytes::from_string(topic.to_owned()));
    t.partitions = offsets
        .iter()
        .take(4096)
        .map(|(partition, offset)| {
            let mut p = OffsetCommitRequestPartition::default();
            p.partition_index = *partition;
            p.committed_offset = *offset;
            p.committed_leader_epoch = -1;
            p.committed_metadata = None;
            p
        })
        .collect();
    let mut r = OffsetCommitRequest::default();
    r.group_id = GroupId(StrBytes::from_string(group.to_owned()));
    r.generation_id_or_member_epoch = -1;
    r.member_id = StrBytes::from_static_str("");
    r.group_instance_id = None;
    r.retention_time_ms = -1;
    r.topics = vec![t];
    Request::OffsetCommit(r)
}

/// OffsetDelete: forget a group's commits on `partitions` of `topic`.
#[must_use]
pub fn offset_delete(group: &str, topic: &str, partitions: &[i32]) -> Request {
    let mut t = OffsetDeleteRequestTopic::default();
    t.name = TopicName(StrBytes::from_string(topic.to_owned()));
    t.partitions = partitions
        .iter()
        .take(4096)
        .map(|p| {
            let mut x = OffsetDeleteRequestPartition::default();
            x.partition_index = *p;
            x
        })
        .collect();
    let mut r = OffsetDeleteRequest::default();
    r.group_id = GroupId(StrBytes::from_string(group.to_owned()));
    r.topics = vec![t];
    Request::OffsetDelete(r)
}

/// DeleteGroups for one group.
#[must_use]
pub fn delete_group(group: &str) -> Request {
    let mut r = DeleteGroupsRequest::default();
    r.groups_names = vec![GroupId(StrBytes::from_string(group.to_owned()))];
    Request::DeleteGroups(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_describe_names_at_most_a_hundred_groups() {
        let names: Vec<String> = (0..150).map(|i| format!("g{i}")).collect();
        let Request::DescribeGroups(r) = describe_groups(&names) else {
            panic!("not DescribeGroups");
        };
        assert_eq!(r.groups.len(), MOST_DESCRIBED);
        let Request::ConsumerGroupDescribe(r) = consumer_group_describe(&names) else {
            panic!("not ConsumerGroupDescribe");
        };
        assert_eq!(r.group_ids.len(), MOST_DESCRIBED);
    }

    #[test]
    fn an_offset_fetch_asks_for_every_partition_of_one_group_and_waits_for_a_stable_answer() {
        let Request::OffsetFetch(r) = offsets_of("g") else {
            panic!("not OffsetFetch");
        };
        assert!(r.require_stable);
        assert_eq!(r.groups.len(), 1);
        assert!(r.groups[0].topics.is_none());
    }

    #[test]
    fn a_reset_is_a_commit_that_names_no_generation_and_no_member() {
        let Request::OffsetCommit(r) = admin_commit("g", "orders", &[(0, 5), (1, 7)]) else {
            panic!("not OffsetCommit");
        };
        assert_eq!(r.generation_id_or_member_epoch, -1);
        assert_eq!(r.member_id.as_str(), "");
        assert!(r.group_instance_id.is_none());
        let got: Vec<(i32, i64)> = r.topics[0]
            .partitions
            .iter()
            .map(|p| (p.partition_index, p.committed_offset))
            .collect();
        assert_eq!(got, vec![(0, 5), (1, 7)]);
    }

    #[test]
    fn the_deletes_name_what_they_delete() {
        let Request::OffsetDelete(r) = offset_delete("g", "orders", &[0, 2]) else {
            panic!("not OffsetDelete");
        };
        assert_eq!(r.topics[0].partitions.len(), 2);
        let Request::DeleteGroups(r) = delete_group("g") else {
            panic!("not DeleteGroups");
        };
        assert_eq!(r.groups_names.len(), 1);
    }
}
