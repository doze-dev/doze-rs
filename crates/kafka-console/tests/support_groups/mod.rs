//! Members and commits for the live group tests: real JoinGroup, SyncGroup, ConsumerGroupHeartbeat and OffsetCommit requests
//! put to the in-process broker, the way clients put them.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use bytes::Bytes;
use kafka_console::backend::requests::admin_commit;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::join_group_request::JoinGroupRequestProtocol;
use kafka_wire::messages::sync_group_request::SyncGroupRequestAssignment;
use kafka_wire::messages::{
    ConsumerGroupHeartbeatRequest, GroupId, JoinGroupRequest, JoinGroupResponse, SyncGroupRequest,
    TopicName,
};

use crate::support_write::World;

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

/// A classic subscription (version 0) to `topics`.
pub fn subscription(topics: &[&str]) -> Vec<u8> {
    let mut b = vec![0, 0];
    b.extend(i32::try_from(topics.len()).unwrap().to_be_bytes());
    for t in topics {
        b.extend(i16::try_from(t.len()).unwrap().to_be_bytes());
        b.extend(t.as_bytes());
    }
    b.extend((-1_i32).to_be_bytes());
    b
}

/// A classic assignment (version 0).
pub fn assignment(topics: &[(&str, &[i32])]) -> Vec<u8> {
    let mut b = vec![0, 0];
    b.extend(i32::try_from(topics.len()).unwrap().to_be_bytes());
    for (t, ps) in topics {
        b.extend(i16::try_from(t.len()).unwrap().to_be_bytes());
        b.extend(t.as_bytes());
        b.extend(i32::try_from(ps.len()).unwrap().to_be_bytes());
        for p in *ps {
            b.extend(p.to_be_bytes());
        }
    }
    b.extend((-1_i32).to_be_bytes());
    b
}

pub fn join_request(
    group: &str,
    member: &str,
    assignor: &str,
    topics: &[&str],
) -> JoinGroupRequest {
    let mut req = JoinGroupRequest::default();
    req.group_id = GroupId(text(group));
    req.member_id = text(member);
    req.session_timeout_ms = 45_000;
    req.rebalance_timeout_ms = 60_000;
    req.protocol_type = text("consumer");
    let mut p = JoinGroupRequestProtocol::default();
    p.name = text(assignor);
    p.metadata = Bytes::from(subscription(topics));
    req.protocols = vec![p];
    req
}

/// A member joins (JoinGroup v3, which hands an id out at once) and is the only one: its answer is the generation.
pub async fn join(w: &World, group: &str, assignor: &str, topics: &[&str]) -> JoinGroupResponse {
    let Response::JoinGroup(r) = w
        .ask(
            3,
            Request::JoinGroup(join_request(group, "", assignor, topics)),
        )
        .await
    else {
        panic!("not JoinGroup")
    };
    assert_eq!(r.error_code, 0, "{r:?}");
    r
}

/// The leader syncs, giving `assignments` (member, bytes).
pub async fn sync(
    w: &World,
    group: &str,
    member: &str,
    generation: i32,
    assignments: &[(&str, Vec<u8>)],
) {
    let mut req = SyncGroupRequest::default();
    req.group_id = GroupId(text(group));
    req.member_id = text(member);
    req.generation_id = generation;
    req.assignments = assignments
        .iter()
        .map(|(m, a)| {
            let mut x = SyncGroupRequestAssignment::default();
            x.member_id = text(m);
            x.assignment = Bytes::from(a.clone());
            x
        })
        .collect();
    let Response::SyncGroup(r) = w.ask(3, Request::SyncGroup(req)).await else {
        panic!("not SyncGroup")
    };
    assert_eq!(r.error_code, 0, "{r:?}");
}

/// A stable classic group of one member holding the given partitions.
pub async fn stable_group(
    w: &World,
    group: &str,
    assignor: &str,
    held: &[(&str, &[i32])],
) -> String {
    let topics: Vec<&str> = held.iter().map(|(t, _)| *t).collect();
    let j = join(w, group, assignor, &topics).await;
    let id = j.member_id.to_string();
    sync(w, group, &id, j.generation_id, &[(&id, assignment(held))]).await;
    id
}

/// A commit with no generation (an administrator's), which Kafka accepts for a group with no members.
pub async fn commit(w: &World, group: &str, topic: &str, offsets: &[(i32, i64)]) {
    let Response::OffsetCommit(r) = w.ask(9, admin_commit(group, topic, offsets)).await else {
        panic!("not OffsetCommit")
    };
    for t in &r.topics {
        for p in &t.partitions {
            assert_eq!(p.error_code, 0, "{r:?}");
        }
    }
}

/// A consumer of the new protocol joins `group` as `member` and heartbeats until it has its assignment.
pub async fn join_848(w: &World, group: &str, member: &str, topics: &[&str]) {
    let mut req = ConsumerGroupHeartbeatRequest::default();
    req.group_id = GroupId(text(group));
    req.member_id = text(member);
    req.member_epoch = 0;
    req.rebalance_timeout_ms = 60_000;
    req.subscribed_topic_names = Some(topics.iter().map(|t| TopicName(text(t))).collect());
    req.topic_partitions = Some(Vec::new());
    let Response::ConsumerGroupHeartbeat(r) = w.ask(0, Request::ConsumerGroupHeartbeat(req)).await
    else {
        panic!("not ConsumerGroupHeartbeat")
    };
    assert_eq!(r.error_code, 0, "{r:?}");
}

/// Writes `n` small records to a partition.
pub async fn produce(w: &World, topic: &str, partition: i32, n: usize) {
    use kafka_wire::batch::build::{BatchBuilder, Rec};
    use kafka_wire::messages::ProduceRequest;
    use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
    let recs: Vec<Rec> = (0..n)
        .map(|i| Rec {
            timestamp_delta: i64::try_from(i).unwrap(),
            offset_delta: None,
            key: Some(b"k".to_vec()),
            value: Some(b"v".to_vec()),
            headers: vec![],
        })
        .collect();
    let batch = BatchBuilder::from_records(kafka_console::source::now_ms() - 10_000, &recs)
        .last_offset_delta(i32::try_from(n).unwrap() - 1)
        .attributes(0)
        .build();
    let mut p = PartitionProduceData::default();
    p.index = partition;
    p.records = Some(batch);
    let mut t = TopicProduceData::default();
    t.name = TopicName(text(topic));
    t.partition_data = vec![p];
    let mut r = ProduceRequest::default();
    r.acks = -1;
    r.timeout_ms = 5000;
    r.topic_data = vec![t];
    let Response::Produce(r) = w.ask(9, Request::Produce(r)).await else {
        panic!("not Produce")
    };
    assert_eq!(r.responses[0].partition_responses[0].error_code, 0);
}

/// What a group committed on a topic, by partition (OffsetFetch as a client asks it).
pub async fn committed(w: &World, group: &str) -> Vec<(String, i32, i64)> {
    let Response::OffsetFetch(r) = w
        .ask(9, kafka_console::backend::requests::offsets_of(group))
        .await
    else {
        panic!("not OffsetFetch")
    };
    r.groups
        .iter()
        .flat_map(|g| &g.topics)
        .flat_map(|t| {
            t.partitions
                .iter()
                .filter(|p| p.committed_offset >= 0)
                .map(|p| (t.name.0.to_string(), p.partition_index, p.committed_offset))
        })
        .collect()
}
