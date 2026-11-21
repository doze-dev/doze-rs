//! The rest of patch 6 (vendor/kafka-protocol/PATCHES.md): the fields Kafka 4.4.0 added to ApiVersions v5, DeleteGroups v3,
//! DescribeLogDirs v5, and the topic ids of OffsetCommit and OffsetFetch v10, each written and read back where it is, and
//! not on the wire where it is not.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use bytes::{Bytes, BytesMut};
use kafka_protocol::messages::delete_groups_response::DeletableGroupResult;
use kafka_protocol::messages::describe_log_dirs_response::DescribeLogDirsResult;
use kafka_protocol::messages::offset_commit_request::{
    OffsetCommitRequestPartition, OffsetCommitRequestTopic,
};
use kafka_protocol::messages::offset_fetch_request::{
    OffsetFetchRequestGroup, OffsetFetchRequestTopics,
};
use kafka_protocol::messages::{
    ApiKey, ApiVersionsRequest, DeleteGroupsResponse, DescribeLogDirsResponse, GroupId,
    OffsetCommitRequest, OffsetFetchRequest, TopicName,
};
use kafka_protocol::protocol::{Decodable, Encodable, StrBytes};
use uuid::Uuid;

fn bytes<T: Encodable>(m: &T, version: i16) -> Bytes {
    let mut out = BytesMut::new();
    m.encode(&mut out, version).unwrap();
    assert_eq!(out.len(), m.compute_size(version).unwrap(), "v{version}");
    out.freeze()
}

fn back<T: Decodable>(mut b: Bytes, version: i16) -> T {
    let m = T::decode(&mut b, version).unwrap();
    assert!(b.is_empty(), "v{version}: bytes left over");
    m
}

const ID: Uuid = Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);

#[test]
fn the_keys_are_at_kafka_4_4_versions() {
    let range = |k: ApiKey| (k.valid_versions().min, k.valid_versions().max);
    // From v0 since patch 8 (the older profiles).
    assert_eq!(range(ApiKey::ListOffsets), (0, 11));
    assert_eq!(range(ApiKey::ApiVersions), (0, 5));
    assert_eq!(range(ApiKey::DeleteGroups), (0, 3));
    assert_eq!(range(ApiKey::DescribeLogDirs), (0, 5));
    assert_eq!(range(ApiKey::OffsetCommit), (0, 10));
    assert_eq!(range(ApiKey::OffsetFetch), (0, 10));
}

#[test]
fn api_versions_v5_carries_the_cluster_and_node_and_v4_does_not() {
    let mut q = ApiVersionsRequest::default();
    q.client_software_name = StrBytes::from_static_str("c");
    q.client_software_version = StrBytes::from_static_str("1");
    q.cluster_id = Some(StrBytes::from_static_str("cluster"));
    q.node_id = 7;
    let r: ApiVersionsRequest = back(bytes(&q, 5), 5);
    assert_eq!((r.cluster_id.as_deref(), r.node_id), (Some("cluster"), 7));
    // Ignorable: below v5 neither is written, and a reader of v4 sees the defaults.
    assert_eq!(bytes(&q, 5).len(), bytes(&q, 4).len() + 1 + 7 + 4);
    let r: ApiVersionsRequest = back(bytes(&q, 4), 4);
    assert_eq!((r.cluster_id, r.node_id), (None, -1));
}

#[test]
fn delete_groups_v3_carries_an_error_message() {
    let mut g = DeletableGroupResult::default();
    g.group_id = GroupId(StrBytes::from_static_str("g"));
    g.error_code = 69;
    g.error_message = Some(StrBytes::from_static_str("why"));
    let mut resp = DeleteGroupsResponse::default();
    resp.results = vec![g];
    let r: DeleteGroupsResponse = back(bytes(&resp, 3), 3);
    assert_eq!(r.results[0].error_message.as_deref(), Some("why"));
    let r: DeleteGroupsResponse = back(bytes(&resp, 2), 2);
    assert_eq!(r.results[0].error_message, None);
}

#[test]
fn describe_log_dirs_v5_says_whether_a_directory_is_cordoned() {
    let mut d = DescribeLogDirsResult::default();
    d.log_dir = StrBytes::from_static_str("/data");
    d.is_cordoned = true;
    let mut resp = DescribeLogDirsResponse::default();
    resp.results = vec![d];
    let r: DescribeLogDirsResponse = back(bytes(&resp, 5), 5);
    assert!(r.results[0].is_cordoned);
    let r: DescribeLogDirsResponse = back(bytes(&resp, 4), 4);
    assert!(!r.results[0].is_cordoned);
}

#[test]
fn offset_commit_and_fetch_v10_name_topics_by_id_and_v9_by_name() {
    let mut p = OffsetCommitRequestPartition::default();
    p.partition_index = 1;
    p.committed_offset = 9;
    let mut t = OffsetCommitRequestTopic::default();
    t.name = TopicName(StrBytes::from_static_str("orders"));
    t.topic_id = ID;
    t.partitions = vec![p];
    let mut req = OffsetCommitRequest::default();
    req.group_id = GroupId(StrBytes::from_static_str("g"));
    req.topics = vec![t];
    let r: OffsetCommitRequest = back(bytes(&req, 10), 10);
    assert_eq!(
        (r.topics[0].topic_id, r.topics[0].name.0.as_str()),
        (ID, "")
    );
    let r: OffsetCommitRequest = back(bytes(&req, 9), 9);
    assert_eq!(
        (r.topics[0].topic_id, r.topics[0].name.0.as_str()),
        (Uuid::nil(), "orders")
    );

    let mut ft = OffsetFetchRequestTopics::default();
    ft.name = TopicName(StrBytes::from_static_str("orders"));
    ft.topic_id = ID;
    ft.partition_indexes = vec![0, 1];
    let mut g = OffsetFetchRequestGroup::default();
    g.group_id = GroupId(StrBytes::from_static_str("g"));
    g.topics = Some(vec![ft]);
    let mut freq = OffsetFetchRequest::default();
    freq.groups = vec![g];
    let r: OffsetFetchRequest = back(bytes(&freq, 10), 10);
    let topic = &r.groups[0].topics.as_ref().unwrap()[0];
    assert_eq!((topic.topic_id, topic.name.0.as_str()), (ID, ""));
    let r: OffsetFetchRequest = back(bytes(&freq, 9), 9);
    let topic = &r.groups[0].topics.as_ref().unwrap()[0];
    assert_eq!(
        (topic.topic_id, topic.name.0.as_str()),
        (Uuid::nil(), "orders")
    );
}
