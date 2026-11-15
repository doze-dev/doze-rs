//! DescribeLogDirs (API 35): the one log directory, each partition's size (its log files), lag 0, never a future log, and the
//! volume's size and room (behaviours T-DLD-*). Checked against real Kafka 4.4.0 by `crates/kafka-wire/examples/oracle_diff.rs`
//! and `kafka-log-dirs.sh`.
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
use kafka_wire::messages::describe_log_dirs_request::DescribableLogDirTopic;
use kafka_wire::messages::{DescribeLogDirsRequest, DescribeLogDirsResponse};
use support::acls::{ALLOW, CLUSTER, DESCRIBE, LITERAL, closed_world, creation, grant, user};

use support::{Rig, records, rig, rig_with, topic, wire_name};

fn request(asks: Option<&[(&str, &[i32])]>) -> DescribeLogDirsRequest {
    let mut req = DescribeLogDirsRequest::default();
    req.topics = asks.map(|asks| {
        asks.iter()
            .map(|(name, parts)| {
                let mut t = DescribableLogDirTopic::default();
                t.topic = wire_name(name);
                t.partitions = parts.to_vec();
                t
            })
            .collect()
    });
    req
}

async fn ask(
    rig: &Rig,
    who: &Caller,
    version: i16,
    req: DescribeLogDirsRequest,
) -> DescribeLogDirsResponse {
    let Response::DescribeLogDirs(r) = rig
        .ask_as(who, version, Request::DescribeLogDirs(req))
        .await
    else {
        panic!("not DescribeLogDirs")
    };
    r
}

/// Each topic listed, with (partition, size, lag, future).
fn listed(r: &DescribeLogDirsResponse) -> Vec<(String, Vec<(i32, i64, i64, bool)>)> {
    r.results[0]
        .topics
        .iter()
        .map(|t| {
            let parts = t
                .partitions
                .iter()
                .map(|p| {
                    (
                        p.partition_index,
                        p.partition_size,
                        p.offset_lag,
                        p.is_future_key,
                    )
                })
                .collect();
            (t.name.0.to_string(), parts)
        })
        .collect()
}

#[tokio::test]
async fn every_partition_is_listed_with_its_log_bytes_and_one_never_written_is_empty() {
    let rig = rig();
    rig.create(vec![topic("orders", 3), topic("audit", 1)], false)
        .await;
    let batch = records(4);
    let size = i64::try_from(batch.len()).unwrap();
    assert_eq!(
        rig.produce(-1, "orders", 1, batch.clone()).await.error_code,
        0
    );
    assert_eq!(rig.produce(-1, "orders", 1, batch).await.error_code, 0);
    for version in 1..=4 {
        let r = ask(&rig, &support::caller(), version, request(None)).await;
        assert_eq!(r.error_code, 0);
        assert_eq!(r.results.len(), 1);
        assert_eq!(r.results[0].log_dir.as_str(), support::DATA);
        assert_eq!(
            listed(&r),
            vec![
                ("audit".to_owned(), vec![(0, 0, 0, false)]),
                (
                    "orders".to_owned(),
                    vec![(0, 0, 0, false), (1, 2 * size, 0, false), (2, 0, 0, false)]
                )
            ],
            "v{version}"
        );
    }
}

#[tokio::test]
async fn only_the_partitions_asked_for_and_a_topic_with_none_left_is_left_out() {
    let rig = rig();
    rig.create(vec![topic("orders", 3)], false).await;
    let asks: &[(&str, &[i32])] = &[("orders", &[2, 0, 9]), ("ghost", &[0]), ("orders", &[2])];
    let r = ask(&rig, &support::caller(), 4, request(Some(asks))).await;
    assert_eq!(
        listed(&r),
        vec![(
            "orders".to_owned(),
            vec![(0, 0, 0, false), (2, 0, 0, false)]
        )]
    );
    let none = ask(&rig, &support::caller(), 4, request(Some(&[]))).await;
    assert!(listed(&none).is_empty());
}

#[tokio::test]
async fn the_volume_says_its_size_and_room_from_v4_and_minus_one_where_the_disk_does_not_say() {
    let rig = rig();
    let r = ask(&rig, &support::caller(), 4, request(None)).await;
    assert_eq!(
        (r.results[0].total_bytes, r.results[0].usable_bytes),
        (-1, -1)
    );
    rig.fs.set_capacity(Some(1 << 30));
    let r = ask(&rig, &support::caller(), 4, request(None)).await;
    let dir = &r.results[0];
    assert_eq!(dir.total_bytes, 1 << 30);
    assert!(
        dir.usable_bytes > 0 && dir.usable_bytes < dir.total_bytes,
        "{dir:?}"
    );
}

#[tokio::test]
async fn describe_on_the_cluster_is_needed_and_without_it_no_directory_is_told() {
    let rig = rig_with(closed_world);
    rig.create_as_admin(vec![topic("orders", 1)], false).await;
    let refused = ask(&rig, &user("someone"), 4, request(None)).await;
    assert_eq!(
        refused.error_code,
        ResponseError::ClusterAuthorizationFailed.code()
    );
    assert!(refused.results.is_empty());
    // At v1 and v2 the answer has no error field: the refusal is just no directory (the request still encodes).
    let old = ask(&rig, &user("someone"), 2, request(None)).await;
    assert!(old.results.is_empty());
    grant(
        &rig,
        vec![creation(
            (CLUSTER, "kafka-cluster", LITERAL),
            "User:ops",
            DESCRIBE,
            ALLOW,
        )],
    )
    .await;
    let allowed = ask(&rig, &user("ops"), 4, request(None)).await;
    assert_eq!(allowed.error_code, 0);
    assert_eq!(
        listed(&allowed).len(),
        1,
        "every topic: DescribeLogDirs asks nothing of the topic's ACLs"
    );
}
