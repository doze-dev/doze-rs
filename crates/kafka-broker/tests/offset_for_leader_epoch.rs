//! OffsetForLeaderEpoch (API 23): each partition has had one leader epoch, 0, from its log start (behaviours T-OFLE-*). The
//! answers are those real Kafka 4.4.0 gives for the same requests (`crates/kafka-wire/examples/oracle_diff.rs`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_broker::Caller;
use kafka_wire::ResponseError;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::delete_records_request::{DeleteRecordsPartition, DeleteRecordsTopic};
use kafka_wire::messages::offset_for_leader_epoch_request::{
    OffsetForLeaderPartition, OffsetForLeaderTopic,
};
use kafka_wire::messages::offset_for_leader_epoch_response::EpochEndOffset;
use kafka_wire::messages::{BrokerId, DeleteRecordsRequest, OffsetForLeaderEpochRequest};
use support::acls::{CLUSTER, DESCRIBE, LITERAL, ALLOW, closed_world, creation, grant, on, user};
use support::produce::records;
use support::{Rig, rig, rig_with, topic, wire_name};

/// (partition, current leader epoch, epoch asked about)
type Ask = (i32, i32, i32);

fn request(asks: &[(&str, &[Ask])], replica: i32) -> OffsetForLeaderEpochRequest {
    let mut req = OffsetForLeaderEpochRequest::default();
    req.replica_id = BrokerId(replica);
    for (name, parts) in asks {
        let mut t = OffsetForLeaderTopic::default();
        t.topic = wire_name(name);
        for &(partition, current, epoch) in *parts {
            let mut p = OffsetForLeaderPartition::default();
            p.partition = partition;
            p.current_leader_epoch = current;
            p.leader_epoch = epoch;
            t.partitions.push(p);
        }
        req.topics.push(t);
    }
    req
}

/// Each topic of the answer, in order, with its partitions as (partition, code, epoch, end offset).
async fn ask(
    rig: &Rig,
    who: &Caller,
    version: i16,
    req: OffsetForLeaderEpochRequest,
) -> Vec<(String, Vec<(i32, i16, i32, i64)>)> {
    let Response::OffsetForLeaderEpoch(r) = rig
        .ask_as(who, version, Request::OffsetForLeaderEpoch(req))
        .await
    else {
        panic!("not OffsetForLeaderEpoch")
    };
    r.topics
        .into_iter()
        .map(|t| {
            let parts = t
                .partitions
                .iter()
                .map(|p: &EpochEndOffset| (p.partition, p.error_code, p.leader_epoch, p.end_offset))
                .collect();
            (t.topic.0.to_string(), parts)
        })
        .collect()
}

async fn with_records(rig: &Rig) {
    rig.create(vec![topic("orders", 2)], false).await;
    for _ in 0..3 {
        assert_eq!(rig.produce(-1, "orders", 0, records(2)).await.error_code, 0);
    }
}

const UNKNOWN: i16 = ResponseError::UnknownTopicOrPartition.code();
const FENCED: i16 = ResponseError::FencedLeaderEpoch.code();
const UNKNOWN_EPOCH: i16 = ResponseError::UnknownLeaderEpoch.code();

#[tokio::test]
async fn epoch_0_ends_at_the_log_end_and_an_epoch_never_had_is_not_found() {
    let rig = rig();
    with_records(&rig).await;
    let anyone = support::caller();
    for version in 2..=4 {
        let got = ask(
            &rig,
            &anyone,
            version,
            request(
                &[(
                    "orders",
                    &[(0, -1, 0), (1, -1, 0), (0, -1, 1), (0, -1, -1), (0, -1, -2)],
                )],
                -1,
            ),
        )
        .await;
        assert_eq!(
            got,
            vec![(
                "orders".to_owned(),
                vec![
                    (0, 0, 0, 6),
                    (1, 0, 0, 0),
                    (0, 0, -1, -1),
                    (0, 0, -1, -1),
                    (0, 0, -2, 0)
                ]
            )],
            "v{version}"
        );
    }
}

#[tokio::test]
async fn the_epoch_the_client_believes_in_is_held_to_the_leaders() {
    let rig = rig();
    with_records(&rig).await;
    let got = ask(
        &rig,
        &support::caller(),
        4,
        request(&[("orders", &[(0, 0, 0), (0, 1, 0), (0, -2, 0), (0, -9, 3)])], -1),
    )
    .await;
    assert_eq!(
        got[0].1,
        vec![
            (0, 0, 0, 6),
            (0, UNKNOWN_EPOCH, -1, -1),
            (0, FENCED, -1, -1),
            (0, FENCED, -1, -1)
        ]
    );
}

#[tokio::test]
async fn an_unknown_topic_or_partition_is_said_to_be_one() {
    let rig = rig();
    with_records(&rig).await;
    let got = ask(
        &rig,
        &support::caller(),
        4,
        request(&[("orders", &[(7, -1, 0), (-1, -1, 0)]), ("nothing", &[(0, 5, 0)])], -1),
    )
    .await;
    assert_eq!(
        got,
        vec![
            ("orders".to_owned(), vec![(7, UNKNOWN, -1, -1), (-1, UNKNOWN, -1, -1)]),
            ("nothing".to_owned(), vec![(0, UNKNOWN, -1, -1)])
        ]
    );
}

#[tokio::test]
async fn after_the_start_moves_the_epoch_begins_there() {
    let rig = rig();
    with_records(&rig).await;
    let mut del = DeleteRecordsRequest::default();
    let mut t = DeleteRecordsTopic::default();
    t.name = wire_name("orders");
    let mut p = DeleteRecordsPartition::default();
    p.partition_index = 0;
    p.offset = 4;
    t.partitions.push(p);
    del.topics.push(t);
    let Response::DeleteRecords(r) = rig.ask(2, Request::DeleteRecords(del)).await else {
        panic!("not DeleteRecords")
    };
    assert_eq!(r.topics[0].partitions[0].error_code, 0);
    let got = ask(
        &rig,
        &support::caller(),
        4,
        request(&[("orders", &[(0, -1, 0), (0, -1, -2)])], -1),
    )
    .await;
    assert_eq!(got[0].1, vec![(0, 0, 0, 6), (0, 0, -2, 4)]);
}

#[tokio::test]
async fn describe_on_the_topic_or_cluster_action_lets_a_caller_ask_and_the_refused_come_last() {
    let rig = rig_with(closed_world);
    rig.create_as_admin(vec![topic("open", 1), topic("shut", 1)], false)
        .await;
    grant(&rig, vec![on("open", "User:reader", DESCRIBE)]).await;
    let asks: &[(&str, &[Ask])] = &[("shut", &[(0, -1, 0)]), ("open", &[(0, -1, 0)])];
    let denied = ResponseError::TopicAuthorizationFailed.code();
    let got = ask(&rig, &user("reader"), 4, request(asks, -1)).await;
    assert_eq!(
        got,
        vec![
            ("open".to_owned(), vec![(0, 0, 0, 0)]),
            ("shut".to_owned(), vec![(0, denied, -1, -1)])
        ]
    );
    // A follower's right (CLUSTER_ACTION on the cluster) covers every topic, whatever the replica id says.
    grant(
        &rig,
        vec![creation((CLUSTER, "kafka-cluster", LITERAL), "User:replica", 9, ALLOW)],
    )
    .await;
    for replica in [-1, 1] {
        let got = ask(&rig, &user("replica"), 4, request(asks, replica)).await;
        assert!(got.iter().all(|(_, parts)| parts[0].1 == 0), "{got:?}");
    }
    // Nobody else is told anything, even that a topic does not exist.
    let got = ask(&rig, &user("stranger"), 4, request(&[("ghost", &[(0, -1, 0)])], 1)).await;
    assert_eq!(got[0].1, vec![(0, denied, -1, -1)]);
}
