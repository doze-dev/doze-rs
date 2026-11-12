//! The topic APIs under ACLs (slice 8d-1), one table for all of them: for each API, the operation it needs on the topic
//! (source: `KafkaApis.scala` 4.3.1, `ControllerApis.scala`, `ConfigHelper.scala`; cited where each API is tested in
//! `authz_topics_*.rs`), the five things every row must do, and the hidden existence of a topic.
//!
//! The rows: CreateTopics CREATE, DeleteTopics DELETE (and DESCRIBE), Produce WRITE, Fetch READ, ListOffsets DESCRIBE,
//! DeleteRecords DELETE, CreatePartitions ALTER, DescribeConfigs DESCRIBE_CONFIGS, AlterConfigs and IncrementalAlterConfigs
//! ALTER_CONFIGS, DescribeProducers READ.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
#[path = "support/authz_topics.rs"]
mod topics;

use kafka_broker::Caller;
use support::Rig;
use support::acls::{
    ALL, ALLOW, CLUSTER, CREATE, DENY, DESCRIBE, LITERAL, PREFIXED, READ, WRITE, admin,
    closed_world, creation, user,
};
use topics::*;

const ALTER: i8 = 7;
const TOPIC_RESOURCE: i8 = 2;
const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;

#[derive(Clone, Copy, Debug)]
enum Api {
    CreateTopics,
    DeleteTopics,
    Produce,
    Fetch,
    ListOffsets,
    DeleteRecords,
    CreatePartitions,
    DescribeConfigs,
    AlterConfigs,
    IncrementalAlterConfigs,
    DescribeProducers,
}

const APIS: [Api; 11] = [
    Api::CreateTopics,
    Api::DeleteTopics,
    Api::Produce,
    Api::Fetch,
    Api::ListOffsets,
    Api::DeleteRecords,
    Api::CreatePartitions,
    Api::DescribeConfigs,
    Api::AlterConfigs,
    Api::IncrementalAlterConfigs,
    Api::DescribeProducers,
];

impl Api {
    /// The operation on the topic the API needs.
    fn op(self) -> i8 {
        match self {
            Api::CreateTopics => CREATE,
            Api::DeleteTopics | Api::DeleteRecords => DELETE,
            Api::Produce => WRITE,
            Api::Fetch | Api::DescribeProducers => READ,
            Api::ListOffsets => DESCRIBE,
            Api::CreatePartitions => ALTER,
            Api::DescribeConfigs => DESCRIBE_CONFIGS,
            Api::AlterConfigs | Api::IncrementalAlterConfigs => ALTER_CONFIGS,
        }
    }

    /// Two topic names for the API to act on: CreateTopics needs ones that are not there yet.
    fn names(self) -> (&'static str, &'static str) {
        match self {
            Api::CreateTopics => ("c", "f"),
            _ => ("t", "u"),
        }
    }

    /// An operation that does not imply `self.op()`.
    fn other(self) -> i8 {
        if self.op() == CREATE { READ } else { CREATE }
    }

    /// The code the API answers for its one item, asking about `topic`.
    async fn code(self, rig: &Rig, who: &Caller, topic: &str) -> i16 {
        match self {
            Api::CreateTopics => {
                let made = support::topic(topic, 1);
                rig.create_topics_as(who, 7, vec![made], false).await[0].error_code
            }
            Api::DeleteTopics => rig.delete_topics_as(who, 5, &[topic]).await[0].error_code,
            Api::Produce => Rig::produced(&rig.produce_to_as(who, 9, -1, &[(topic, 0)]).await)[0].2,
            Api::Fetch => {
                let r = rig
                    .fetch_topics_as(who, 12, &[(topic, 0, 0, 1 << 20)])
                    .await;
                Rig::fetched(&r)[0].2
            }
            Api::ListOffsets => {
                rig.list_offsets_as(who, 7, &[(topic, &[0])]).await.topics[0].partitions[0]
                    .error_code
            }
            Api::DeleteRecords => {
                rig.delete_records_as(who, 2, &[(topic, &[0])]).await.topics[0].partitions[0]
                    .error_code
            }
            Api::CreatePartitions => rig.create_partitions_as(who, 3, &[(topic, 2)]).await[0].1,
            Api::DescribeConfigs => {
                rig.describe_configs_as(who, 4, &[(TOPIC_RESOURCE, topic)])
                    .await[0]
                    .error_code
            }
            Api::AlterConfigs => rig.alter_as(who, &[(TOPIC_RESOURCE, topic)]).await[0].0,
            Api::IncrementalAlterConfigs => {
                rig.incremental_as(who, &[(TOPIC_RESOURCE, topic)]).await[0].0
            }
            Api::DescribeProducers => {
                rig.describe_producers_as(who, &[(topic, &[0])])
                    .await
                    .topics[0]
                    .partitions[0]
                    .error_code
            }
        }
    }
}

/// A closed-world rig holding topics `t` and `u`, one record in each.
async fn world() -> Rig {
    rig_with_topics(&["t", "u"]).await
}

#[tokio::test]
async fn a_caller_with_the_operation_is_served_and_one_with_nothing_is_refused_with_29() {
    for api in APIS {
        let rig = world().await;
        let (name, _) = api.names();
        grant(&rig, vec![allow(name, "User:alice", api.op())]).await;
        assert_eq!(api.code(&rig, &user("alice"), name).await, 0, "{api:?}");
        // bob has no ACL on the topic, which has ACLs: refused, and the topic is not changed by it (the next test).
        assert_eq!(
            api.code(&rig, &user("bob"), name).await,
            TOPIC_AUTHORIZATION_FAILED,
            "{api:?}"
        );
    }
}

#[tokio::test]
async fn an_operation_that_does_not_imply_the_needed_one_is_not_enough() {
    for api in APIS {
        let rig = world().await;
        let (name, _) = api.names();
        grant(&rig, vec![allow(name, "User:alice", api.other())]).await;
        assert_eq!(
            api.code(&rig, &user("alice"), name).await,
            TOPIC_AUTHORIZATION_FAILED,
            "{api:?} needs {} and alice has {}",
            api.op(),
            api.other()
        );
    }
}

#[tokio::test]
async fn an_acl_on_another_topic_is_not_enough() {
    for api in APIS {
        let rig = world().await;
        let (name, free) = api.names();
        grant(&rig, vec![allow(free, "User:alice", api.op())]).await;
        assert_eq!(
            api.code(&rig, &user("alice"), name).await,
            TOPIC_AUTHORIZATION_FAILED,
            "{api:?}"
        );
    }
}

#[tokio::test]
async fn a_super_user_is_served_though_a_deny_names_it() {
    for api in APIS {
        let rig = world().await;
        let (name, _) = api.names();
        grant(
            &rig,
            vec![
                deny(name, "User:admin", ALL),
                deny(name, "User:admin", api.op()),
            ],
        )
        .await;
        assert_eq!(api.code(&rig, &admin(), name).await, 0, "{api:?}");
    }
}

#[tokio::test]
async fn a_deny_beats_an_allow_whichever_is_stored_first() {
    for api in APIS {
        let rig = world().await;
        let (name, _) = api.names();
        grant(
            &rig,
            vec![
                allow(name, "User:alice", api.op()),
                deny(name, "User:alice", api.op()),
                deny(name, "User:bob", api.op()),
                allow(name, "User:bob", api.op()),
            ],
        )
        .await;
        for who in ["alice", "bob"] {
            assert_eq!(
                api.code(&rig, &user(who), name).await,
                TOPIC_AUTHORIZATION_FAILED,
                "{api:?} {who}"
            );
        }
    }
}

#[tokio::test]
async fn a_deny_of_all_beats_an_allow_of_the_operation() {
    for api in APIS {
        let rig = world().await;
        let (name, _) = api.names();
        grant(
            &rig,
            vec![
                allow(name, "User:alice", api.op()),
                deny(name, "User:alice", ALL),
            ],
        )
        .await;
        assert_eq!(
            api.code(&rig, &user("alice"), name).await,
            TOPIC_AUTHORIZATION_FAILED,
            "{api:?}"
        );
    }
}

#[tokio::test]
async fn the_any_user_a_prefix_and_a_wildcard_name_are_honoured() {
    for api in APIS {
        let (name, _) = api.names();
        let prefix = &name[..1];
        for who in ["anyone", "carol", "dave"] {
            // (A fresh broker for each: what the first one did is not to be undone by the next.)
            let rig = world().await;
            grant(
                &rig,
                vec![
                    allow(name, "User:*", api.op()),
                    allow_prefixed(prefix, "User:carol", api.op()),
                    allow("*", "User:dave", api.op()),
                ],
            )
            .await;
            assert_eq!(api.code(&rig, &user(who), name).await, 0, "{api:?} {who}");
        }
        // A PREFIXED deny over the prefix beats a literal allow.
        let rig = world().await;
        grant(
            &rig,
            vec![
                allow(name, "User:alice", api.op()),
                topic_acl(prefix, PREFIXED, "User:alice", api.op(), DENY),
            ],
        )
        .await;
        assert_eq!(
            api.code(&rig, &user("alice"), name).await,
            TOPIC_AUTHORIZATION_FAILED,
            "{api:?}"
        );
    }
}

#[tokio::test]
async fn the_default_applies_only_to_a_topic_no_acl_covers() {
    for api in APIS {
        // allow.everyone.if.no.acl.found is true, so a topic without an ACL is open to all.
        let rig = support::rig_with(|c| {
            c.super_users.insert("User:admin".to_owned());
        });
        let (covered, free) = api.names();
        let setup: Vec<_> = ["t", "u"].iter().map(|n| support::topic(n, 1)).collect();
        rig.create_topics_as(&admin(), 7, setup, false).await;
        grant(&rig, vec![allow(covered, "User:alice", api.other())]).await;
        cover_the_cluster(&rig, api).await;
        assert_eq!(
            api.code(&rig, &user("bob"), covered).await,
            TOPIC_AUTHORIZATION_FAILED,
            "{api:?}: an ACL for anybody turns the default off for the topic"
        );
        assert_eq!(
            api.code(&rig, &user("bob"), free).await,
            0,
            "{api:?}: nothing covers the other"
        );
    }
}

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

/// CreateTopics and DeleteTopics ask the cluster first, and with the default allowing, a cluster no ACL covers is open to
/// everyone, which covers every topic (`hasClusterAuth`, `ControllerApis.createTopics` and `.deleteTopics`). An ACL for
/// somebody else on the cluster turns that off, so that the topic's own ACLs decide.
async fn cover_the_cluster(rig: &Rig, api: Api) {
    let op = match api {
        Api::CreateTopics => CREATE,
        Api::DeleteTopics => DELETE,
        _ => return,
    };
    let acl = creation(
        (CLUSTER, "kafka-cluster", LITERAL),
        "User:nobody",
        op,
        ALLOW,
    );
    grant(rig, vec![acl]).await;
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
