//! A committed offset carries more than a number: the leader epoch the client saw comes back with it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::Duration;

use support::{rig_with, topic};

async fn rig_with_topic() -> support::Rig {
    let rig = rig_with(|c| c.group_initial_rebalance_delay = Duration::ZERO);
    rig.create(vec![topic("t", 3)], false).await;
    rig
}

#[tokio::test]
async fn the_leader_epoch_a_client_commits_comes_back_with_the_offset() {
    use kafka_wire::StrBytes;
    use kafka_wire::api::{Request, Response};
    use kafka_wire::messages::offset_commit_request::{
        OffsetCommitRequestPartition, OffsetCommitRequestTopic,
    };
    use kafka_wire::messages::{GroupId, OffsetCommitRequest, OffsetFetchRequest, TopicName};
    let rig = rig_with_topic().await;
    let mut partition = OffsetCommitRequestPartition::default();
    partition.partition_index = 0;
    partition.committed_offset = 5;
    partition.committed_leader_epoch = 7;
    let mut topic = OffsetCommitRequestTopic::default();
    topic.name = TopicName(StrBytes::from_static_str("t"));
    topic.partitions.push(partition);
    let mut commit = OffsetCommitRequest::default();
    commit.group_id = GroupId(StrBytes::from_static_str("g"));
    commit.generation_id_or_member_epoch = -1;
    commit.topics.push(topic);
    rig.ask(8, Request::OffsetCommit(commit)).await;
    let mut fetch = OffsetFetchRequest::default();
    fetch.group_id = GroupId(StrBytes::from_static_str("g"));
    fetch.topics = None; // everything the group has
    let Response::OffsetFetch(r) = rig.ask(7, Request::OffsetFetch(fetch)).await else {
        panic!("not OffsetFetch")
    };
    assert_eq!(r.topics[0].partitions[0].committed_leader_epoch, 7);
}
