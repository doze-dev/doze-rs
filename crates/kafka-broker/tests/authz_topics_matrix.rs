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

#[path = "support/authz_api.rs"]
mod api;
mod support;
#[path = "support/authz_topics.rs"]
mod topics;

use api::*;
use support::acls::{ALL, CREATE, DENY, DESCRIBE, PREFIXED, READ, WRITE, admin, user};
use topics::*;

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
async fn the_weaker_right_that_the_stronger_one_implies_is_not_enough() {
    // READ, WRITE, DELETE and ALTER each imply DESCRIBE, and ALTER_CONFIGS implies DESCRIBE_CONFIGS; the implication runs
    // one way. (A handler that asked for the weaker right would pass every caller who holds the stronger.)
    for api in APIS {
        let weaker = match api.op() {
            DESCRIBE | DESCRIBE_CONFIGS | CREATE => continue,
            ALTER_CONFIGS => DESCRIBE_CONFIGS,
            _ => DESCRIBE,
        };
        let rig = world().await;
        let (name, _) = api.names();
        grant(&rig, vec![allow(name, "User:alice", weaker)]).await;
        assert_eq!(
            api.code(&rig, &user("alice"), name).await,
            TOPIC_AUTHORIZATION_FAILED,
            "{api:?} needs {} and alice has only {weaker}",
            api.op()
        );
    }
}

#[tokio::test]
async fn the_stronger_right_that_implies_the_needed_one_is_enough() {
    // DescribeConfigs is granted by ALTER_CONFIGS; ListOffsets by any of READ, WRITE, DELETE and ALTER.
    for (api, stronger) in [
        (Api::DescribeConfigs, ALTER_CONFIGS),
        (Api::ListOffsets, READ),
        (Api::ListOffsets, WRITE),
        (Api::ListOffsets, DELETE),
        (Api::ListOffsets, ALTER),
    ] {
        let rig = world().await;
        grant(&rig, vec![allow("t", "User:alice", stronger)]).await;
        assert_eq!(
            api.code(&rig, &user("alice"), "t").await,
            0,
            "{api:?} with {stronger}"
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
