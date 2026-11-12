//! CreatePartitions and DeleteTopics under ACLs (slice 8d-1): the shape of each denial, the order of the answers and what is
//! hidden. Every row is from the Kafka 4.3.1 source, not from a recording: `ControllerApis.scala` `createPartitions` (826-861)
//! and `deleteTopics` (222-360).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
#[path = "support/authz_topics.rs"]
mod topics;

use support::acls::{ALLOW, CLUSTER, DESCRIBE, LITERAL, READ, admin, creation, user};
use topics::*;

const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;
const ALTER: i8 = 7;

fn on_cluster(principal: &str, op: i8) -> kafka_wire::messages::create_acls_request::AclCreation {
    creation((CLUSTER, "kafka-cluster", LITERAL), principal, op, ALLOW)
}

// CreatePartitions

#[tokio::test]
async fn create_partitions_needs_alter_and_answers_the_refused_after_the_duplicates_with_no_message()
 {
    // ControllerApis.createPartitions: duplicates (INVALID_REQUEST "Duplicate topic name.", both removed), then the
    // unauthorized (TOPIC_AUTHORIZATION_FAILED, no message), then the controller's results.
    for version in 0..=3 {
        let rig = rig_with_topics(&["a", "b", "c", "d"]).await;
        grant(
            &rig,
            vec![
                allow("a", "User:alice", ALTER),
                allow("c", "User:alice", ALTER),
            ],
        )
        .await;
        let r = rig
            .create_partitions_as(
                &user("alice"),
                version,
                &[
                    ("a", 2),
                    ("b", 2),
                    ("c", 2),
                    ("d", 2),
                    ("d", 3),
                    ("ghost", 2),
                ],
            )
            .await;
        let seen: Vec<_> = r.iter().map(|x| (x.0.as_str(), x.1)).collect();
        assert_eq!(
            seen,
            [("d", 42), ("b", 29), ("ghost", 29), ("a", 0), ("c", 0)],
            "v{version}: duplicates, refused, granted"
        );
        assert_eq!(r[0].2.as_deref(), Some("Duplicate topic name."));
        assert_eq!(r[1].2, None, "the refusal has no message");
    }
}

#[tokio::test]
async fn create_partitions_with_describe_or_read_only_is_refused_and_changes_nothing() {
    let rig = rig_with_topics(&["a"]).await;
    grant(
        &rig,
        vec![
            allow("a", "User:alice", READ),
            allow("a", "User:alice", DESCRIBE),
        ],
    )
    .await;
    let r = rig
        .create_partitions_as(&user("alice"), 3, &[("a", 5)])
        .await;
    assert_eq!((r[0].1, r[0].2.clone()), (29, None));
    let t = rig.metadata_as(&admin(), 12, Some(&["a"]), false).await;
    assert_eq!(t.topics[0].partitions.len(), 1);
}

// DeleteTopics

const NIL: kafka_wire::Uuid = kafka_wire::Uuid::nil();

#[tokio::test]
async fn delete_topics_by_name_hides_what_a_caller_may_not_describe() {
    // ControllerApis.deleteTopics: by name, not describable -> 29 with the name, whether the topic exists or not; present,
    // describable and not deletable -> 29 with the name; absent and describable -> UNKNOWN_TOPIC_OR_PARTITION. The id
    // is zero in every one of them, and the message is "Topic authorization failed.".
    let rig = rig_with_topics(&["seen", "hidden", "mine"]).await;
    grant(
        &rig,
        vec![
            allow("seen", "User:alice", READ),
            allow("hidden", "User:bob", READ),
            allow("mine", "User:alice", DELETE),
            allow("ghost", "User:alice", READ),
        ],
    )
    .await;
    // The refusal is writable at every version: the name is there from v1, the message from v5, the id from v6.
    for version in [1, 2, 3, 4, 5, 6] {
        let r = rig
            .delete_topics_as(&user("stranger"), version, &["seen", "ghost"])
            .await;
        assert_eq!(
            r.iter().map(|x| x.error_code).collect::<Vec<_>>(),
            [29, 29],
            "v{version}"
        );
    }
    let r = rig
        .delete_topics_as(
            &user("alice"),
            5,
            &["seen", "hidden", "nothing", "ghost", "mine"],
        )
        .await;
    let seen: Vec<_> = r
        .iter()
        .map(|x| (x.name.as_ref().map(|n| n.0.to_string()), x.error_code))
        .collect();
    assert_eq!(
        seen,
        [
            (Some("seen".into()), 29),
            (Some("hidden".into()), 29),
            (Some("nothing".into()), 29),
            (Some("ghost".into()), UNKNOWN_TOPIC_OR_PARTITION),
            (Some("mine".into()), 0),
        ]
    );
    assert!(r[0].topic_id.is_nil() && r[1].topic_id.is_nil());
    assert_eq!(
        r[0].error_message.as_deref(),
        Some("Topic authorization failed.")
    );
}

#[tokio::test]
async fn delete_topics_by_id_shows_the_name_only_to_a_caller_that_may_describe_it() {
    let rig = rig_with_topics(&["seen", "hidden", "mine"]).await;
    grant(
        &rig,
        vec![
            allow("seen", "User:alice", READ),
            allow("mine", "User:alice", DELETE),
        ],
    )
    .await;
    let ids = [
        rig.id_of("seen").await,
        rig.id_of("hidden").await,
        rig.id_of("mine").await,
    ];
    let r = rig.delete_topic_ids_as(&user("alice"), &ids).await;
    let names: Vec<_> = r
        .iter()
        .map(|x| x.name.as_ref().map(|n| n.0.to_string()))
        .collect();
    assert_eq!(names, [Some("seen".into()), None, Some("mine".into())]);
    assert_eq!(
        r.iter().map(|x| x.error_code).collect::<Vec<_>>(),
        [29, 29, 0]
    );
    assert_eq!(
        r.iter().map(|x| x.topic_id).collect::<Vec<_>>(),
        ids,
        "the id is the caller's own; it is no secret"
    );
    // Nothing but "mine" is gone.
    let left = rig.metadata_as(&admin(), 12, None, false).await;
    let mut names: Vec<_> = support::acls::shown(&left)
        .into_iter()
        .map(|t| t.0)
        .collect();
    names.sort();
    assert_eq!(names, ["hidden", "seen"]);
    let _ = NIL;
}

#[tokio::test]
async fn delete_on_the_cluster_grants_both_describing_and_deleting() {
    let rig = rig_with_topics(&["x", "y"]).await;
    grant(&rig, vec![on_cluster("User:ops", DELETE)]).await;
    let r = rig.delete_topics_as(&user("ops"), 5, &["x", "ghost"]).await;
    assert_eq!(
        r.iter().map(|x| x.error_code).collect::<Vec<_>>(),
        [0, UNKNOWN_TOPIC_OR_PARTITION]
    );
    // A DENY of DELETE on the topic does not undo the cluster's grant: the cluster is asked first.
    grant(&rig, vec![deny("y", "User:ops", DELETE)]).await;
    assert_eq!(
        rig.delete_topics_as(&user("ops"), 5, &["y"]).await[0].error_code,
        0
    );
}

#[tokio::test]
async fn delete_implies_describe_but_a_deny_of_describe_hides_the_topic_from_the_deleter() {
    let rig = rig_with_topics(&["x"]).await;
    grant(
        &rig,
        vec![
            allow("x", "User:alice", DELETE),
            deny("x", "User:alice", DESCRIBE),
        ],
    )
    .await;
    // Describe is denied, so by name the answer is the same 29 a stranger gets (and the topic is still there).
    let r = rig.delete_topics_as(&user("alice"), 5, &["x"]).await;
    assert_eq!(r[0].error_code, 29);
    assert_eq!(
        support::acls::shown(&rig.metadata_as(&admin(), 12, None, false).await).len(),
        1
    );
}

/// `RCM.java` 1025-1028: an id nobody holds is UNKNOWN_TOPIC_ID (100) with Kafka's own words, to a caller that may
/// delete everything and to one that may delete nothing; a name nobody holds stays 3 (SEC-D84).
#[tokio::test]
async fn delete_topics_by_an_unknown_id_is_100_and_by_an_unknown_name_is_3() {
    let rig = rig_with_topics(&["t"]).await;
    let ghost = kafka_wire::Uuid::from_u128(0xfeed_f00d);
    for who in [admin(), user("nobody")] {
        let r = rig.delete_topic_ids_as(&who, &[ghost]).await;
        assert_eq!(r[0].error_code, 100, "{who:?}");
        assert_eq!(r[0].topic_id, ghost);
        assert!(r[0].name.is_none());
        assert_eq!(
            r[0].error_message.as_deref(),
            Some("This server does not host this topic ID.")
        );
    }
    let r = rig.delete_topics_as(&admin(), 5, &["ghost"]).await;
    assert_eq!(r[0].error_code, UNKNOWN_TOPIC_OR_PARTITION);
}
