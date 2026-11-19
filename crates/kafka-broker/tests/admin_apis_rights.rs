//! UpdateFeatures (ADM-15) and the right each admin API of debt K3 asks (SEC-341 to SEC-346). Held to real Kafka 4.4.0 by
//! `crates/kafka-wire/examples/oracle_admin`.
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
