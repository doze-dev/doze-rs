//! CreateTopics, CreatePartitions and DeleteTopics under ACLs (slice 8d-1): the shape of each denial, the order of the
//! answers and what is hidden. Every row is from the Kafka 4.3.1 source, not from a recording:
//! `ControllerApis.scala` `createTopics` (362-449), `createPartitions` (796-861), `deleteTopics` (198-360);
//! `ReplicationControlManager.java` 815-843.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
#[path = "support/authz_topics.rs"]
mod topics;

use support::acls::{ALLOW, CLUSTER, CREATE, DESCRIBE, LITERAL, READ, admin, creation, user};
use support::{assigned, topic};
use topics::*;

const TOPIC_ALREADY_EXISTS: i16 = 36;
const INVALID_TOPIC: i16 = 17;
const INVALID_REQUEST: i16 = 42;
const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;
const ALTER: i8 = 7;

fn on_cluster(principal: &str, op: i8) -> kafka_wire::messages::create_acls_request::AclCreation {
    creation((CLUSTER, "kafka-cluster", LITERAL), principal, op, ALLOW)
}

fn summary(
    r: &[kafka_wire::messages::create_topics_response::CreatableTopicResult],
) -> Vec<(String, i16)> {
    r.iter()
        .map(|t| (t.name.0.to_string(), t.error_code))
        .collect()
}

// CreateTopics

#[tokio::test]
async fn an_unauthorized_topic_is_29_with_kafkas_text_and_no_numbers_and_comes_last() {
    // ControllerApis.createTopics: the unauthorized names are taken out before the controller sees the request and added
    // after its answers (and the duplicates') with TOPIC_AUTHORIZATION_FAILED "Authorization failed.".
    let rig = rig_with_topics(&[]).await;
    grant(
        &rig,
        vec![
            allow("ok1", "User:alice", CREATE),
            allow("ok2", "User:alice", CREATE),
        ],
    )
    .await;
    for version in 2..=7 {
        let r = rig
            .create_topics_as(
                &user("alice"),
                version,
                vec![
                    topic("no1", 1),
                    topic(&format!("ok1-{version}"), 1),
                    topic("no2", 1),
                    topic(&format!("ok2-{version}"), 1),
                ],
                false,
            )
            .await;
        // alice's ACLs are on "ok1" and "ok2" exactly, so the suffixed names are not covered: all four are refused,
        // and in request order (the set order of Kafka is unspecified).
        assert_eq!(r.len(), 4, "v{version}");
        assert!(r.iter().all(|t| t.error_code == 29), "v{version}: {r:?}");
        assert_eq!(r[0].error_message.as_deref(), Some("Authorization failed."));
        assert_eq!((r[0].num_partitions, r[0].replication_factor), (-1, -1));
        assert!(r[0].topic_id.is_nil());
    }
    let r = rig
        .create_topics_as(
            &user("alice"),
            7,
            vec![
                topic("no1", 1),
                topic("ok1", 1),
                topic("no2", 1),
                topic("ok2", 1),
            ],
            false,
        )
        .await;
    assert_eq!(
        summary(&r),
        [
            ("ok1".into(), 0),
            ("ok2".into(), 0),
            ("no1".into(), 29),
            ("no2".into(), 29)
        ],
        "what was made first, in the order asked, then the refusals"
    );
    // The refused were not made.
    let seen = rig.metadata_as(&admin(), 12, None, false).await;
    let mut names: Vec<_> = support::acls::shown(&seen)
        .into_iter()
        .map(|t| t.0)
        .collect();
    names.sort();
    assert_eq!(names, ["ok1", "ok2"]);
}

#[tokio::test]
async fn a_name_that_is_taken_or_invalid_is_not_revealed_to_a_caller_that_may_not_create_it() {
    let rig = rig_with_topics(&["taken"]).await;
    grant(
        &rig,
        vec![
            allow("mine", "User:alice", CREATE),
            allow("taken", "User:bob", CREATE),
        ],
    )
    .await;
    let alice = user("alice");
    // alice may create "mine" only. "taken" exists, "bad name" is invalid, and neither is told to her.
    let r = rig
        .create_topics_as(
            &alice,
            7,
            vec![topic("taken", 1), topic("bad name", 1), topic("mine", 1)],
            false,
        )
        .await;
    assert_eq!(
        summary(&r),
        [
            ("mine".into(), 0),
            ("taken".into(), 29),
            ("bad name".into(), 29)
        ]
    );
    // bob may create "taken": and is told it exists. A caller with the right is told the truth about an invalid name.
    let r = rig
        .create_topics_as(&user("bob"), 7, vec![topic("taken", 1)], false)
        .await;
    assert_eq!(summary(&r), [("taken".into(), TOPIC_ALREADY_EXISTS)]);
    let r = rig
        .create_topics_as(&admin(), 7, vec![topic("bad name", 1)], false)
        .await;
    assert_eq!(summary(&r), [("bad name".into(), INVALID_TOPIC)]);
}

#[tokio::test]
async fn a_duplicate_name_is_answered_42_before_authorization_is_asked() {
    // ControllerApis.createTopics: "Duplicated names never reach authz" (they are set aside first).
    let rig = rig_with_topics(&[]).await;
    let r = rig
        .create_topics_as(
            &user("nobody"),
            7,
            vec![topic("d", 1), topic("d", 1), topic("e", 1)],
            false,
        )
        .await;
    let by_name: std::collections::BTreeMap<_, _> = summary(&r).into_iter().collect();
    assert_eq!(
        by_name["d"], INVALID_REQUEST,
        "a duplicate is a malformed request, whoever asks"
    );
    assert_eq!(by_name["e"], 29);
    assert_eq!(r.len(), 2, "one answer for the duplicated name");
}

#[tokio::test]
async fn validate_only_asks_for_the_same_right() {
    let rig = rig_with_topics(&[]).await;
    grant(&rig, vec![allow("v", "User:alice", CREATE)]).await;
    let ask = |who: &str, name: &'static str| {
        let (rig, who) = (&rig, user(who));
        async move {
            rig.create_topics_as(&who, 7, vec![topic(name, 1)], true)
                .await[0]
                .error_code
        }
    };
    assert_eq!(ask("alice", "v").await, 0);
    assert_eq!(ask("bob", "v").await, 29);
    assert!(support::acls::shown(&rig.metadata_as(&admin(), 12, None, false).await).is_empty());
}

#[tokio::test]
async fn without_describe_configs_the_topic_is_made_and_its_configs_are_withheld() {
    // ReplicationControlManager.createTopic: authorizedToReturnConfigs is DESCRIBE_CONFIGS on the topic; without it
    // TopicConfigErrorCode is TOPIC_AUTHORIZATION_FAILED and no configs, partition count or replication factor are
    // returned (the defaults, -1).
    let rig = rig_with_topics(&[]).await;
    grant(
        &rig,
        vec![
            allow("plain", "User:alice", CREATE),
            allow("seen", "User:bob", CREATE),
            allow("seen", "User:bob", DESCRIBE_CONFIGS),
            allow("alt", "User:carol", CREATE),
            allow("alt", "User:carol", ALTER_CONFIGS),
        ],
    )
    .await;
    let r = rig
        .create_topics_as(&user("alice"), 7, vec![topic("plain", 3)], false)
        .await;
    let t = &r[0];
    assert_eq!(t.error_code, 0);
    assert_eq!(t.topic_config_error_code, 29);
    assert_eq!((t.num_partitions, t.replication_factor), (-1, -1));
    assert!(t.configs.as_ref().is_none_or(Vec::is_empty));
    assert!(!t.topic_id.is_nil(), "the id is told: the topic was made");
    // It was made in full, whatever alice was told.
    assert_eq!(
        rig.metadata_as(&admin(), 12, Some(&["plain"]), false)
            .await
            .topics[0]
            .partitions
            .len(),
        3
    );
    // With DESCRIBE_CONFIGS (granted itself, or by ALTER_CONFIGS which implies it) the answer is whole.
    for (who, name) in [("bob", "seen"), ("carol", "alt")] {
        let t = &rig
            .create_topics_as(&user(who), 7, vec![topic(name, 2)], false)
            .await[0];
        assert_eq!((t.error_code, t.topic_config_error_code), (0, 0), "{who}");
        assert_eq!((t.num_partitions, t.replication_factor), (2, 1), "{who}");
        assert!(t.configs.as_ref().is_some_and(|c| !c.is_empty()), "{who}");
    }
}

#[tokio::test]
async fn create_on_the_cluster_covers_every_name_and_deny_on_the_topic_does_not_undo_it() {
    let rig = rig_with_topics(&[]).await;
    grant(
        &rig,
        vec![
            on_cluster("User:maker", CREATE),
            deny("blocked", "User:maker", CREATE),
        ],
    )
    .await;
    // CREATE on the cluster is asked first and is enough (`hasClusterAuth`): the topic's DENY is never asked.
    let r = rig
        .create_topics_as(
            &user("maker"),
            7,
            vec![topic("blocked", 1), topic("any", 1)],
            false,
        )
        .await;
    assert_eq!(summary(&r), [("blocked".into(), 0), ("any".into(), 0)]);
    // A DENY on the cluster takes it away, and then the topic's own ACLs decide.
    grant(
        &rig,
        vec![creation(
            (CLUSTER, "kafka-cluster", LITERAL),
            "User:maker",
            CREATE,
            support::acls::DENY,
        )],
    )
    .await;
    let r = rig
        .create_topics_as(&user("maker"), 7, vec![topic("again", 1)], false)
        .await;
    assert_eq!(summary(&r), [("again".into(), 29)]);
}

#[tokio::test]
async fn a_manual_assignment_and_other_invalid_requests_are_not_judged_for_a_caller_that_may_not_create()
 {
    let rig = rig_with_topics(&[]).await;
    let r = rig
        .create_topics_as(
            &user("nobody"),
            7,
            vec![assigned("x", &[(0, &[9])]), topic("y", 0)],
            false,
        )
        .await;
    assert!(
        r.iter().all(|t| t.error_code == 29),
        "{r:?}: authorization first, then validation"
    );
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
