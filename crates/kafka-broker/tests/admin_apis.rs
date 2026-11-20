//! The admin APIs of debt K3 on one node (behaviours ADM-10 to ADM-14, SEC-340): DescribeTopicPartitions, ElectLeaders,
//! AlterPartitionReassignments, ListPartitionReassignments, AlterReplicaLogDirs and ListConfigResources. Held to real Kafka
//! 4.4.0 by `crates/kafka-wire/examples/oracle_admin`; these pin the same answers here. UpdateFeatures and the rights each API
//! asks: `admin_apis_rights.rs`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines,
    unused_imports,
    reason = "tests: each walks one API through Kafka's cases in turn; the imports are the helpers' shared set"
)]

mod support;

use kafka_broker::Caller;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::alter_partition_reassignments_request::{
    ReassignablePartition, ReassignableTopic,
};
use kafka_wire::messages::alter_replica_log_dirs_request::{
    AlterReplicaLogDir, AlterReplicaLogDirTopic,
};
use kafka_wire::messages::describe_topic_partitions_request::{Cursor, TopicRequest};
use kafka_wire::messages::elect_leaders_request::TopicPartitions;
use kafka_wire::messages::list_partition_reassignments_request::ListPartitionReassignmentsTopics;
use kafka_wire::messages::update_features_request::FeatureUpdateKey;
use kafka_wire::messages::{
    AlterPartitionReassignmentsRequest, AlterReplicaLogDirsRequest, BrokerId,
    DescribeTopicPartitionsRequest, DescribeTopicPartitionsResponse, ElectLeadersRequest,
    ListConfigResourcesRequest, ListPartitionReassignmentsRequest, UpdateFeaturesRequest,
};
use kafka_wire::{ResponseError, StrBytes};
use support::acls::{
    ALLOW, ALTER, CLUSTER, DESCRIBE, LITERAL, closed_world, creation, grant, on, user,
};
use support::{Rig, rig, rig_with, topic, wire_name};

use support::admin::*;

#[tokio::test]
async fn describe_topic_partitions_pages_in_name_order_and_says_where_to_go_on() {
    let rig = fixture().await;
    let me = support::caller();
    let all = describe(&rig, &me, dtp(&[], 2000, None)).await;
    assert_eq!(
        pages(&all),
        (
            own(&[
                ("many", 0, &[0, 1, 2, 3, 4]),
                ("one", 0, &[0]),
                ("three", 0, &[0, 1, 2])
            ]),
            None
        )
    );
    let p = &all.topics[0].partitions[0];
    assert_eq!(
        (
            p.leader_id.0,
            p.leader_epoch,
            p.replica_nodes.len(),
            p.isr_nodes.len()
        ),
        (1, 0, 1, 1)
    );
    assert_eq!(p.eligible_leader_replicas.as_deref(), Some(&[][..]));
    assert!(all.topics[0].topic_authorized_operations > 0);
    // A topic cut short ends the page at its next partition; a full page before a topic, at that topic's 0.
    let r = describe(&rig, &me, dtp(&[], 2, None)).await;
    assert_eq!(
        pages(&r),
        (own(&[("many", 0, &[0, 1])]), Some(("many".into(), 2)))
    );
    let r = describe(&rig, &me, dtp(&[], 4, Some(("many", 3)))).await;
    assert_eq!(
        pages(&r),
        (
            own(&[("many", 0, &[3, 4]), ("one", 0, &[0]), ("three", 0, &[0])]),
            Some(("three".into(), 1))
        )
    );
    // A page filled exactly by a topic: the next topic starts the next page at its partition 0.
    let r = describe(&rig, &me, dtp(&[], 6, None)).await;
    assert_eq!(
        pages(&r),
        (
            own(&[("many", 0, &[0, 1, 2, 3, 4]), ("one", 0, &[0])]),
            Some(("three".into(), 0))
        )
    );
    // Named: each once, missing ones take no room, a name no topic could have is 17.
    let r = describe(
        &rig,
        &me,
        dtp(&["three", "nothing", "bad name!", "three"], 2000, None),
    )
    .await;
    assert_eq!(
        pages(&r).0,
        own(&[
            ("bad name!", 17, &[]),
            ("nothing", 3, &[]),
            ("three", 0, &[0, 1, 2])
        ])
    );
    // A limit of 0 is 1; a cursor past the partitions lists the topic with none.
    let r = describe(&rig, &me, dtp(&["three"], 0, None)).await;
    assert_eq!(
        pages(&r),
        (own(&[("three", 0, &[0])]), Some(("three".into(), 1)))
    );
    let r = describe(&rig, &me, dtp(&["three"], 10, Some(("three", 7)))).await;
    assert_eq!(pages(&r), (own(&[("three", 0, &[])]), None));
    // A cursor whose topic is not named, or whose partition is negative, is 42 for every topic named.
    for (names, cursor) in [
        (&["one"][..], ("three", 0)),
        (&["three"][..], ("three", -1)),
    ] {
        let r = describe(&rig, &me, dtp(names, 10, Some(cursor))).await;
        assert!(
            r.topics
                .iter()
                .all(|t| t.error_code == 42 && t.partitions.is_empty())
        );
        assert_eq!(r.topics.len(), names.len());
    }
}

#[tokio::test]
async fn describe_topic_partitions_hides_what_the_caller_may_not_describe() {
    let rig = rig_with(closed_world);
    create_as_admin(&rig, vec![topic("open", 1), topic("shut", 2)]).await;
    grant(&rig, vec![on("open", "User:alice", DESCRIBE)]).await;
    let alice = user("alice");
    let r = describe(&rig, &alice, dtp(&[], 2000, None)).await;
    assert_eq!(pages(&r).0, own(&[("open", 0, &[0])]));
    let r = describe(&rig, &alice, dtp(&["shut", "open", "ghost"], 2000, None)).await;
    assert_eq!(
        pages(&r).0,
        own(&[("open", 0, &[0]), ("ghost", 29, &[]), ("shut", 29, &[])])
    );
    assert!(r.topics[1].topic_id.is_nil() && r.topics[2].topic_id.is_nil());
    assert_eq!(r.topics[1].topic_authorized_operations, i32::MIN);
}

#[tokio::test]
async fn no_election_is_ever_needed_on_one_node() {
    let rig = fixture().await;
    let me = support::caller();
    let asks: &[(&str, &[i32])] = &[("three", &[0, 9]), ("ghost", &[0])];
    for version in 0..=2 {
        let (_, lines) = elect_codes(&rig, &me, version, elect(0, Some(asks))).await;
        assert_eq!(
            lines,
            vec![
                (
                    "three".into(),
                    0,
                    84,
                    Some("Leader election not needed for topic partition.".into())
                ),
                (
                    "three".into(),
                    9,
                    3,
                    Some("No such partition as three-9".into())
                ),
                ("ghost".into(), 0, 3, Some("No such topic as ghost".into())),
            ],
            "v{version}"
        );
        // Every partition: every topic, none of its partitions.
        let Response::ElectLeaders(r) = rig
            .ask_as(&me, version, Request::ElectLeaders(elect(0, None)))
            .await
        else {
            panic!()
        };
        assert_eq!(r.replica_election_results.len(), 3);
        assert!(
            r.replica_election_results
                .iter()
                .all(|t| t.partition_result.is_empty())
        );
    }
    let (top, lines) = elect_codes(&rig, &me, 2, elect(5, Some(asks))).await;
    assert_eq!(top, 42);
    assert!(
        lines
            .iter()
            .all(|l| l.2 == 42 && l.3.as_deref() == Some("Unknown election type 5"))
    );
    assert_eq!(
        elect_codes(&rig, &me, 2, elect(5, Some(&[]))).await,
        (0, vec![])
    );
}

#[tokio::test]
async fn a_reassignment_to_this_broker_is_done_at_once_and_any_other_is_refused_in_kafkas_words() {
    let rig = fixture().await;
    let me = support::caller();
    let asks: &[(&str, i32, Option<&[i32]>)] = &[
        ("three", 0, Some(&[1])),
        ("three", 1, None),
        ("three", 2, Some(&[])),
        ("many", 0, Some(&[2])),
        ("many", 1, Some(&[1, 1])),
        ("many", 7, Some(&[1])),
        ("ghost", 0, Some(&[1])),
    ];
    let Response::AlterPartitionReassignments(r) = rig
        .ask_as(&me, 1, Request::AlterPartitionReassignments(reassign(asks)))
        .await
    else {
        panic!()
    };
    let got: Vec<(i16, Option<String>)> = (r.responses.iter())
        .flat_map(|t| {
            t.partitions.iter().map(|p| {
                (
                    p.error_code,
                    p.error_message.as_ref().map(ToString::to_string),
                )
            })
        })
        .collect();
    let m = |s: &str| Some(s.to_owned());
    assert_eq!(
        got,
        vec![
            (0, None),
            (85, None),
            (
                39,
                m("The manual partition assignment includes an empty replica list.")
            ),
            (
                39,
                m(
                    "The manual partition assignment includes broker 2, but no such broker is registered."
                )
            ),
            (
                39,
                m("The manual partition assignment includes the broker 1 more than once.")
            ),
            (3, m("Unable to find partition many:7.")),
            (3, m("Unable to find a topic named ghost.")),
        ]
    );
    assert_eq!(
        (
            r.error_code,
            r.error_message.as_ref().map(ToString::to_string)
        ),
        (0, None)
    );
    let mut list = ListPartitionReassignmentsRequest::default();
    let mut t = ListPartitionReassignmentsTopics::default();
    t.name = wire_name("three");
    t.partition_indexes = vec![0, 1];
    list.topics = Some(vec![t]);
    for req in [ListPartitionReassignmentsRequest::default(), list] {
        let Response::ListPartitionReassignments(r) = rig
            .ask_as(&me, 0, Request::ListPartitionReassignments(req))
            .await
        else {
            panic!()
        };
        assert_eq!(
            (r.error_code, r.topics.len(), r.error_message),
            (0, 0, None)
        );
    }
}

#[tokio::test]
async fn a_log_moves_only_to_where_it_is() {
    let rig = fixture().await;
    let long = "l".repeat(220);
    let req = move_logs(&[
        (support::DATA, "three", &[0, 9]),
        ("/elsewhere", "one", &[0]),
        (support::DATA, "ghost", &[0]),
        (support::DATA, long.as_str(), &[0]),
    ]);
    for version in 0..=2 {
        let Response::AlterReplicaLogDirs(r) = rig
            .ask_as(
                &support::caller(),
                version,
                Request::AlterReplicaLogDirs(req.clone()),
            )
            .await
        else {
            panic!()
        };
        let got: Vec<(String, i32, i16)> = (r.results.iter())
            .flat_map(|t| {
                t.partitions
                    .iter()
                    .map(|p| (t.topic_name.0.to_string(), p.partition_index, p.error_code))
            })
            .collect();
        assert_eq!(
            got,
            vec![
                ("three".into(), 0, 0),
                ("three".into(), 9, 3),
                ("one".into(), 0, 57),
                ("ghost".into(), 0, 3),
                (long.clone(), 0, 17),
            ]
        );
    }
}

#[tokio::test]
async fn config_resources_are_listed_by_type_and_an_unknown_type_is_unsupported() {
    let rig = fixture().await;
    let me = support::caller();
    let ask = |types: &[i8], version: i16| {
        let mut req = ListConfigResourcesRequest::default();
        req.resource_types = types.to_vec();
        let rig = &rig;
        let me = &me;
        async move {
            let Response::ListConfigResources(r) = rig
                .ask_as(me, version, Request::ListConfigResources(req))
                .await
            else {
                panic!()
            };
            let rows: Vec<(i8, String)> = r
                .config_resources
                .iter()
                .map(|c| (c.resource_type, c.resource_name.to_string()))
                .collect();
            (r.error_code, rows)
        }
    };
    assert_eq!(ask(&[], 0).await, (0, vec![]));
    assert_eq!(
        ask(&[], 1).await.1,
        vec![
            (8, "1".into()),
            (4, "1".into()),
            (2, "many".into()),
            (2, "one".into()),
            (2, "three".into())
        ]
    );
    assert_eq!(ask(&[4], 1).await.1, vec![(4, "1".into())]);
    assert_eq!(ask(&[2, 99], 1).await, (35, vec![]));
}
