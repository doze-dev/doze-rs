//! The topic APIs under ACLs (slice 8d-1), part two: the cluster as the first thing CreateTopics and DeleteTopics ask, the
//! hidden existence of a topic, and that a refusal changes nothing. Source as in `authz_topics_matrix.rs`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/authz_api.rs"]
mod api;
mod support;
#[path = "support/authz_topics.rs"]
mod topics;

use api::*;

use support::acls::{ALL, ALLOW, CLUSTER, CREATE, LITERAL, READ, admin, creation, user};
use topics::*;

#[tokio::test]
async fn an_acl_for_another_host_turns_the_default_off_without_allowing_this_one() {
    for api in APIS {
        let rig = support::rig_with(|c| {
            c.super_users.insert("User:admin".to_owned());
        });
        let (name, _) = api.names();
        rig.create_topics_as(&admin(), 7, vec![support::topic("t", 1)], false)
            .await;
        let mut elsewhere = allow(name, "User:alice", api.op());
        elsewhere.host = kafka_wire::StrBytes::from_string("9.9.9.9".to_owned());
        grant(&rig, vec![elsewhere]).await;
        cover_the_cluster(&rig, api).await;
        assert_eq!(
            api.code(&rig, &user("alice"), name).await,
            TOPIC_AUTHORIZATION_FAILED,
            "{api:?}"
        );
    }
}

#[tokio::test]
async fn with_the_default_allowing_a_cluster_no_acl_covers_is_open_and_covers_every_topic() {
    let rig = support::rig_with(|c| {
        c.super_users.insert("User:admin".to_owned());
    });
    let setup: Vec<_> = ["t", "u"].iter().map(|n| support::topic(n, 1)).collect();
    rig.create_topics_as(&admin(), 7, setup, false).await;
    // A topic ACL for somebody else changes nothing for CreateTopics and DeleteTopics: the cluster decides first.
    grant(
        &rig,
        vec![
            allow("t", "User:alice", READ),
            allow("n", "User:alice", READ),
        ],
    )
    .await;
    let made = rig
        .create_topics_as(&user("bob"), 7, vec![support::topic("n", 1)], false)
        .await;
    assert_eq!(made[0].error_code, 0);
    assert_eq!(
        rig.delete_topics_as(&user("bob"), 5, &["t"]).await[0].error_code,
        0
    );
    // Once the cluster has an ACL of its own, the default is off for it, and bob has neither right.
    cover_the_cluster(&rig, Api::CreateTopics).await;
    cover_the_cluster(&rig, Api::DeleteTopics).await;
    let made = rig
        .create_topics_as(&user("bob"), 7, vec![support::topic("m", 1)], false)
        .await;
    assert_eq!(
        made[0].error_code, 0,
        "no ACL covers topic m, so the topic default allows"
    );
    grant(&rig, vec![allow("u", "User:alice", READ)]).await;
    assert_eq!(
        rig.delete_topics_as(&user("bob"), 5, &["u"]).await[0].error_code,
        TOPIC_AUTHORIZATION_FAILED
    );
    // CREATE on the cluster is enough for any name, covered or not.
    let on_cluster = creation(
        (CLUSTER, "kafka-cluster", LITERAL),
        "User:maker",
        CREATE,
        ALLOW,
    );
    grant(&rig, vec![on_cluster]).await;
    let made = rig
        .create_topics_as(&user("maker"), 7, vec![support::topic("n2", 1)], false)
        .await;
    assert_eq!(made[0].error_code, 0);
}

#[tokio::test]
async fn a_topic_that_does_not_exist_is_hidden_from_a_caller_that_may_not_use_it() {
    // Authorization is asked before existence, with the operation the API needs: a caller without it learns nothing, and
    // a caller with it is told the topic is not there. (CreateTopics has no absent topic to hide: it makes one.)
    for api in APIS.into_iter().filter(|a| !matches!(a, Api::CreateTopics)) {
        let rig = world().await;
        grant(&rig, vec![allow("ghost", "User:alice", api.op())]).await;
        assert_eq!(
            api.code(&rig, &user("bob"), "ghost").await,
            TOPIC_AUTHORIZATION_FAILED,
            "{api:?}: ghost does not exist and bob may not use it"
        );
        assert_eq!(
            api.code(&rig, &user("alice"), "ghost").await,
            UNKNOWN_TOPIC_OR_PARTITION,
            "{api:?}: alice may, so she may be told"
        );
    }
}

#[tokio::test]
async fn a_refusal_changes_nothing() {
    // Each API that changes something, refused: the topic is as it was.
    let rig = world().await;
    let bob = user("bob");
    grant(&rig, vec![allow("t", "User:alice", ALL)]).await;
    for api in [
        Api::DeleteTopics,
        Api::Produce,
        Api::DeleteRecords,
        Api::CreatePartitions,
        Api::AlterConfigs,
        Api::IncrementalAlterConfigs,
    ] {
        assert_eq!(api.code(&rig, &bob, "t").await, TOPIC_AUTHORIZATION_FAILED);
    }
    let r = rig
        .create_topics_as(&bob, 7, vec![support::topic("n", 1)], false)
        .await;
    assert_eq!(r[0].error_code, TOPIC_AUTHORIZATION_FAILED);
    let seen = rig.metadata_as(&admin(), 12, None, false).await;
    let mut names: Vec<_> = support::acls::shown(&seen);
    names.sort();
    assert_eq!(names, [("t".into(), 0, 1), ("u".into(), 0, 1)]);
    let end = rig.list_offsets_as(&admin(), 7, &[("t", &[0])]).await;
    assert_eq!(
        end.topics[0].partitions[0].offset, 1,
        "nothing was produced or deleted"
    );
    let configs = rig
        .describe_configs_as(&admin(), 4, &[(TOPIC_RESOURCE, "t")])
        .await;
    let retention = configs[0]
        .configs
        .iter()
        .find(|c| &*c.name == "retention.ms")
        .unwrap();
    assert_ne!(retention.value.as_deref(), Some("5000"));
    let _ = (CLUSTER, LITERAL, creation);
}
