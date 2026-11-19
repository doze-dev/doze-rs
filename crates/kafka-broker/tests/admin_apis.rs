//! The admin APIs of debt K3 on one node (behaviours ADM-10 to ADM-16, SEC-340 to SEC-346): DescribeTopicPartitions,
//! ElectLeaders, AlterPartitionReassignments, ListPartitionReassignments, AlterReplicaLogDirs, ListConfigResources and
//! UpdateFeatures. Held to real Kafka 4.4.0 by `crates/kafka-wire/examples/oracle_admin`; these pin the same answers here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
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

const DESCRIBE_CONFIGS: i8 = 10;

/// Topics made by the super user of a closed world.
async fn create_as_admin(
    rig: &Rig,
    topics: Vec<kafka_wire::messages::create_topics_request::CreatableTopic>,
) {
    let mut req = kafka_wire::messages::CreateTopicsRequest::default();
    req.topics = topics;
    let Response::CreateTopics(r) = rig
        .ask_as(&user("admin"), 7, Request::CreateTopics(req))
        .await
    else {
        panic!()
    };
    assert!(r.topics.iter().all(|t| t.error_code == 0));
}

async fn fixture() -> Rig {
    let rig = rig();
    rig.create(
        vec![topic("one", 1), topic("three", 3), topic("many", 5)],
        false,
    )
    .await;
    rig
}

fn dtp(names: &[&str], limit: i32, cursor: Option<(&str, i32)>) -> DescribeTopicPartitionsRequest {
    let mut req = DescribeTopicPartitionsRequest::default();
    req.response_partition_limit = limit;
    req.topics = (names.iter())
        .map(|n| {
            let mut t = TopicRequest::default();
            t.name = wire_name(n);
            t
        })
        .collect();
    req.cursor = cursor.map(|(name, p)| {
        let mut c = Cursor::default();
        c.topic_name = wire_name(name);
        c.partition_index = p;
        c
    });
    req
}

async fn describe(
    rig: &Rig,
    who: &Caller,
    req: DescribeTopicPartitionsRequest,
) -> DescribeTopicPartitionsResponse {
    let Response::DescribeTopicPartitions(r) = rig
        .ask_as(who, 0, Request::DescribeTopicPartitions(req))
        .await
    else {
        panic!("not DescribeTopicPartitions")
    };
    r
}

/// Each topic as (name, error, partitions listed), and the cursor.
fn pages(
    r: &DescribeTopicPartitionsResponse,
) -> (Vec<(String, i16, Vec<i32>)>, Option<(String, i32)>) {
    let topics = (r.topics.iter())
        .map(|t| {
            let name = t.name.as_ref().map_or(String::new(), |n| n.0.to_string());
            (
                name,
                t.error_code,
                t.partitions.iter().map(|p| p.partition_index).collect(),
            )
        })
        .collect();
    let cursor = r
        .next_cursor
        .as_ref()
        .map(|c| (c.topic_name.0.to_string(), c.partition_index));
    (topics, cursor)
}

fn own(v: &[(&str, i16, &[i32])]) -> Vec<(String, i16, Vec<i32>)> {
    v.iter()
        .map(|(n, e, p)| ((*n).to_owned(), *e, p.to_vec()))
        .collect()
}

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

fn elect(kind: i8, asks: Option<&[(&str, &[i32])]>) -> ElectLeadersRequest {
    let mut req = ElectLeadersRequest::default();
    req.election_type = kind;
    req.topic_partitions = asks.map(|asks| {
        asks.iter()
            .map(|(name, parts)| {
                let mut tp = TopicPartitions::default();
                tp.topic = wire_name(name);
                tp.partitions = parts.to_vec();
                tp
            })
            .collect()
    });
    req
}

async fn elect_codes(
    rig: &Rig,
    who: &Caller,
    version: i16,
    req: ElectLeadersRequest,
) -> (i16, Vec<(String, i32, i16, Option<String>)>) {
    let Response::ElectLeaders(r) = rig.ask_as(who, version, Request::ElectLeaders(req)).await
    else {
        panic!("not ElectLeaders")
    };
    let lines = (r.replica_election_results.iter())
        .flat_map(|t| {
            t.partition_result.iter().map(|p| {
                (
                    t.topic.0.to_string(),
                    p.partition_id,
                    p.error_code,
                    p.error_message.as_ref().map(ToString::to_string),
                )
            })
        })
        .collect();
    (r.error_code, lines)
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

fn reassign(asks: &[(&str, i32, Option<&[i32]>)]) -> AlterPartitionReassignmentsRequest {
    let mut req = AlterPartitionReassignmentsRequest::default();
    for (name, part, replicas) in asks {
        let mut t = ReassignableTopic::default();
        t.name = wire_name(name);
        let mut p = ReassignablePartition::default();
        p.partition_index = *part;
        p.replicas = replicas.map(|r| r.iter().map(|&b| BrokerId(b)).collect());
        t.partitions.push(p);
        req.topics.push(t);
    }
    req
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

fn move_logs(asks: &[(&str, &str, &[i32])]) -> AlterReplicaLogDirsRequest {
    let mut req = AlterReplicaLogDirsRequest::default();
    for (dir, name, parts) in asks {
        let mut d = AlterReplicaLogDir::default();
        d.path = StrBytes::from_string((*dir).to_owned());
        let mut t = AlterReplicaLogDirTopic::default();
        t.name = wire_name(name);
        t.partitions = parts.to_vec();
        d.topics.push(t);
        req.dirs.push(d);
    }
    req
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

fn update(asks: &[(&str, i16, i8)], validate_only: bool) -> UpdateFeaturesRequest {
    let mut req = UpdateFeaturesRequest::default();
    req.validate_only = validate_only;
    for (name, level, kind) in asks {
        let mut u = FeatureUpdateKey::default();
        u.feature = StrBytes::from_string((*name).to_owned());
        u.max_version_level = *level;
        u.upgrade_type = *kind;
        req.feature_updates.push(u);
    }
    req
}

async fn features(
    rig: &Rig,
    who: &Caller,
    version: i16,
    req: UpdateFeaturesRequest,
) -> (i16, Option<String>, usize) {
    let Response::UpdateFeatures(r) = rig.ask_as(who, version, Request::UpdateFeatures(req)).await
    else {
        panic!()
    };
    (
        r.error_code,
        r.error_message.map(|m| m.to_string()),
        r.results.len(),
    )
}

#[tokio::test]
async fn feature_updates_are_checked_as_kafka_checks_them_and_levels_do_not_move() {
    let rig = fixture().await;
    let me = support::caller();
    let prefix = "The update failed for all features since the following feature had an error: ";
    // Validated, or no change: success, with results before v2.
    assert_eq!(
        features(&rig, &me, 1, update(&[("transaction.version", 0, 2)], true)).await,
        (0, None, 1)
    );
    assert_eq!(
        features(&rig, &me, 2, update(&[("group.version", 1, 1)], false)).await,
        (0, None, 0)
    );
    let refused = [
        (
            ("transaction.version", 1, 1),
            "Invalid update version 1 for feature transaction.version. Can't downgrade the version of this feature without setting the upgrade type to either safe or unsafe downgrade.",
        ),
        (
            ("transaction.version", 3, 1),
            "Invalid update version 3 for feature transaction.version. Local controller 1 only supports versions 0-2",
        ),
        (
            ("no.such", 1, 1),
            "Invalid update version 1 for feature no.such. Local controller 1 does not support this feature.",
        ),
        (
            ("no.such", 0, 1),
            "Invalid update version 0 for feature no.such. Feature no.such not found.",
        ),
        (
            ("group.version", 0, 9),
            "Invalid update version 0 for feature group.version. The controller does not support the given upgrade type.",
        ),
        (
            ("kraft.version", 1, 2),
            "Invalid update version 1 for feature kraft.version. Can't downgrade to a newer version.",
        ),
        (
            ("metadata.version", 30, 2),
            "Unsupported metadata.version downgrade from 33 to 30. Refusing to perform the requested downgrade because it might delete metadata information.",
        ),
    ];
    for (ask, why) in refused {
        assert_eq!(
            features(&rig, &me, 1, update(&[ask], true)).await,
            (95, Some(format!("{prefix}{why}")), 0),
            "{ask:?}"
        );
    }
    // The order Kafka's hash map checks them in: metadata.version before eligible.leader.replicas.version.
    let (_, why, _) = features(
        &rig,
        &me,
        2,
        update(
            &[
                ("eligible.leader.replicas.version", 1, 1),
                ("metadata.version", 22, 2),
            ],
            true,
        ),
    )
    .await;
    assert!(
        why.unwrap()
            .contains("Unsupported metadata.version downgrade from 33 to 22")
    );
    // A change Kafka would make is refused here (ADM-D9).
    let (code, why, _) = features(
        &rig,
        &me,
        2,
        update(&[("transaction.version", 1, 2)], false),
    )
    .await;
    assert_eq!(code, 95);
    assert!(why.unwrap().contains("--kafka-version"));
}

#[tokio::test]
async fn each_admin_api_asks_kafkas_right_and_refuses_in_kafkas_shape() {
    let rig = rig_with(closed_world);
    create_as_admin(&rig, vec![topic("t", 2)]).await;
    let bob = user("bob");
    let denied = ResponseError::ClusterAuthorizationFailed.code();
    let (top, lines) = elect_codes(&rig, &bob, 2, elect(0, Some(&[("t", &[0, 1])]))).await;
    assert_eq!(top, denied);
    assert!(lines.iter().all(|l| {
        l.2 == denied
            && l.3
                .as_deref()
                .is_some_and(|m| m.ends_with("needs ALTER permission."))
    }));
    assert_eq!(
        elect_codes(&rig, &bob, 0, elect(0, Some(&[("t", &[0])])))
            .await
            .0,
        0,
        "v0 has no top-level code"
    );
    let Response::AlterPartitionReassignments(r) = rig
        .ask_as(
            &bob,
            1,
            Request::AlterPartitionReassignments(reassign(&[("t", 0, Some(&[1]))])),
        )
        .await
    else {
        panic!()
    };
    assert_eq!(
        (r.error_code, r.responses[0].partitions[0].error_code),
        (denied, denied)
    );
    let Response::ListPartitionReassignments(r) = rig
        .ask_as(
            &bob,
            0,
            Request::ListPartitionReassignments(ListPartitionReassignmentsRequest::default()),
        )
        .await
    else {
        panic!()
    };
    assert!(
        r.error_code == denied
            && r.error_message
                .unwrap()
                .ends_with("needs DESCRIBE permission.")
    );
    let Response::AlterReplicaLogDirs(r) = rig
        .ask_as(
            &bob,
            2,
            Request::AlterReplicaLogDirs(move_logs(&[(support::DATA, "t", &[0, 1])])),
        )
        .await
    else {
        panic!()
    };
    assert!(
        r.results[0]
            .partitions
            .iter()
            .all(|p| p.error_code == denied)
    );
    let Response::ListConfigResources(r) = rig
        .ask_as(
            &bob,
            1,
            Request::ListConfigResources(ListConfigResourcesRequest::default()),
        )
        .await
    else {
        panic!()
    };
    assert_eq!((r.error_code, r.config_resources.len()), (denied, 0));
    assert_eq!(
        features(&rig, &bob, 1, update(&[("group.version", 1, 1)], true))
            .await
            .0,
        denied
    );
    // And the rights each asks: ALTER on the cluster, DESCRIBE, DESCRIBE_CONFIGS.
    grant(
        &rig,
        vec![
            creation(
                (CLUSTER, "kafka-cluster", LITERAL),
                "User:bob",
                ALTER,
                ALLOW,
            ),
            creation(
                (CLUSTER, "kafka-cluster", LITERAL),
                "User:bob",
                DESCRIBE_CONFIGS,
                ALLOW,
            ),
        ],
    )
    .await;
    assert_eq!(
        elect_codes(&rig, &bob, 2, elect(0, Some(&[("t", &[0])])))
            .await
            .1[0]
            .2,
        84
    );
    let Response::ListConfigResources(r) = rig
        .ask_as(
            &bob,
            1,
            Request::ListConfigResources(ListConfigResourcesRequest::default()),
        )
        .await
    else {
        panic!()
    };
    assert_eq!(r.error_code, 0);
    // ALTER implies DESCRIBE (SEC-20), so the list is open to bob now.
    let Response::ListPartitionReassignments(r) = rig
        .ask_as(
            &bob,
            0,
            Request::ListPartitionReassignments(ListPartitionReassignmentsRequest::default()),
        )
        .await
    else {
        panic!()
    };
    assert_eq!(r.error_code, 0);
}
