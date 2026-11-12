//! The reads the topic forms make before they ask for a write: the topic's partitions, its configs, where its log
//! starts and ends, who reads it. Each is a protocol request, answered by the same handlers a client meets.

use kafka_wire::StrBytes;
use kafka_wire::api::Request;
use kafka_wire::messages::describe_configs_request::DescribeConfigsResource;
use kafka_wire::messages::list_offsets_request::{ListOffsetsPartition, ListOffsetsTopic};
use kafka_wire::messages::metadata_request::MetadataRequestTopic;
use kafka_wire::messages::offset_fetch_request::{
    OffsetFetchRequestGroup, OffsetFetchRequestTopics,
};
use kafka_wire::messages::{
    BrokerId, DescribeConfigsRequest, DescribeGroupsRequest, GroupId, ListGroupsRequest,
    ListOffsetsRequest, MetadataRequest, OffsetFetchRequest, TopicName,
};

/// The resource type of a topic in DescribeConfigs.
const TOPIC_RESOURCE: i8 = 2;

fn topic_name(s: &str) -> TopicName {
    TopicName(StrBytes::from_string(s.to_owned()))
}

/// Metadata for one topic (its id and partitions), creating nothing.
#[must_use]
pub fn topic_metadata(topic: &str) -> Request {
    let mut t = MetadataRequestTopic::default();
    t.name = Some(topic_name(topic));
    let mut r = MetadataRequest::default();
    r.topics = Some(vec![t]);
    r.allow_auto_topic_creation = false;
    Request::Metadata(r)
}

/// The configs of one topic named, as the topic has them (value and source).
#[must_use]
pub fn topic_configs(topic: &str, keys: &[&str]) -> Request {
    let mut res = DescribeConfigsResource::default();
    res.resource_type = TOPIC_RESOURCE;
    res.resource_name = StrBytes::from_string(topic.to_owned());
    res.configuration_keys = Some(
        keys.iter()
            .map(|k| StrBytes::from_string((*k).to_owned()))
            .collect(),
    );
    let mut r = DescribeConfigsRequest::default();
    r.resources = vec![res];
    Request::DescribeConfigs(r)
}

/// An offset of each of these partitions: `timestamp` -2 is the earliest, -1 the latest, -3 the record with the newest
/// timestamp; `isolation` 1 is `read_committed` (the last stable offset), 0 is `read_uncommitted`.
#[must_use]
pub fn offsets_at(topic: &str, partitions: &[i32], timestamp: i64, isolation: i8) -> Request {
    let mut t = ListOffsetsTopic::default();
    t.name = topic_name(topic);
    t.partitions = partitions
        .iter()
        .map(|p| {
            let mut x = ListOffsetsPartition::default();
            x.partition_index = *p;
            x.current_leader_epoch = -1;
            x.timestamp = timestamp;
            x
        })
        .collect();
    let mut r = ListOffsetsRequest::default();
    r.replica_id = BrokerId(-1);
    r.isolation_level = isolation;
    r.topics = vec![t];
    Request::ListOffsets(r)
}

/// Every consumer group.
#[must_use]
pub fn list_groups() -> Request {
    Request::ListGroups(ListGroupsRequest::default())
}

/// These groups, described (state and members).
#[must_use]
pub fn describe_groups(ids: &[String]) -> Request {
    let mut r = DescribeGroupsRequest::default();
    r.groups = ids
        .iter()
        .map(|g| GroupId(StrBytes::from_string(g.clone())))
        .collect();
    Request::DescribeGroups(r)
}

/// The offsets these groups have committed for these partitions of one topic.
#[must_use]
pub fn committed_offsets(groups: &[String], topic: &str, partitions: &[i32]) -> Request {
    let mut r = OffsetFetchRequest::default();
    r.groups = groups
        .iter()
        .map(|g| {
            let mut t = OffsetFetchRequestTopics::default();
            t.name = topic_name(topic);
            t.partition_indexes = partitions.to_vec();
            let mut grp = OffsetFetchRequestGroup::default();
            grp.group_id = GroupId(StrBytes::from_string(g.clone()));
            grp.member_epoch = -1;
            grp.topics = Some(vec![t]);
            grp
        })
        .collect();
    Request::OffsetFetch(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_metadata_for_one_topic_never_creates_it() {
        let Request::Metadata(r) = topic_metadata("orders") else {
            panic!()
        };
        assert!(!r.allow_auto_topic_creation);
        let topics = r.topics.unwrap();
        assert_eq!(topics.len(), 1);
        assert_eq!(topics[0].name.as_ref().unwrap().0.as_str(), "orders");
    }

    #[test]
    fn offsets_and_configs_ask_for_what_they_say() {
        let Request::ListOffsets(r) = offsets_at("t", &[0, 2], -2, 1) else {
            panic!()
        };
        assert_eq!((r.isolation_level, r.replica_id.0), (1, -1));
        let ps = &r.topics[0].partitions;
        assert_eq!(
            ps.iter()
                .map(|p| (p.partition_index, p.timestamp))
                .collect::<Vec<_>>(),
            [(0, -2), (2, -2)]
        );
        let Request::DescribeConfigs(r) = topic_configs("t", &["cleanup.policy"]) else {
            panic!()
        };
        assert_eq!(
            (
                r.resources[0].resource_type,
                r.resources[0].resource_name.as_str()
            ),
            (2, "t")
        );
        assert_eq!(r.resources[0].configuration_keys.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn committed_offsets_ask_each_group_for_the_one_topic() {
        let Request::OffsetFetch(r) =
            committed_offsets(&["g1".to_owned(), "g2".to_owned()], "orders", &[0, 1])
        else {
            panic!()
        };
        assert_eq!(r.groups.len(), 2);
        let topics = r.groups[1].topics.as_ref().unwrap();
        assert_eq!(
            (
                topics[0].name.0.as_str(),
                topics[0].partition_indexes.clone()
            ),
            ("orders", vec![0, 1])
        );
    }
}
