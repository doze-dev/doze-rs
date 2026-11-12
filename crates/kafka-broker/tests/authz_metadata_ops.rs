//! Metadata's authorized-operations fields under ACLs (slice 8d-1). Source: `KafkaApis.scala` 960-981 (when and for what),
//! `AuthHelper.scala` 62-76 (`authorizedOperations`: one `authorize` per operation an ACL on that resource type can name,
//! a bit `1 << code` for each allowed one, ALL never a bit), `AclEntry.scala` 53-63 (the operations per resource type),
//! `Utils.java` 1367-1372 (`to32BitField`).
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
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::metadata_request::MetadataRequestTopic;
use kafka_wire::messages::{MetadataRequest, MetadataResponse};
use support::acls::{
    ALL, ALLOW, ALTER, CLUSTER, CREATE, DESCRIBE, LITERAL, READ, WRITE, creation, user,
};
use support::{Rig, rig_with, wire_name};
use topics::*;

/// The bit of an operation code.
const fn bit(op: i8) -> i32 {
    1 << op
}

/// Every operation a topic ACL can name: READ, WRITE, CREATE, DELETE, ALTER, DESCRIBE, DESCRIBE_CONFIGS, ALTER_CONFIGS.
const ALL_TOPIC: i32 = bit(3) | bit(4) | bit(5) | bit(6) | bit(7) | bit(8) | bit(10) | bit(11);
/// Every operation a cluster ACL can name: CREATE, ALTER, DESCRIBE, CLUSTER_ACTION, DESCRIBE_CONFIGS, ALTER_CONFIGS,
/// IDEMPOTENT_WRITE.
const ALL_CLUSTER: i32 = bit(5) | bit(7) | bit(8) | bit(9) | bit(10) | bit(11) | bit(12);
/// What the schema says for a field that was not asked for.
const NOT_ASKED: i32 = i32::MIN;

fn cluster_acl(principal: &str, op: i8) -> kafka_wire::messages::create_acls_request::AclCreation {
    creation((CLUSTER, "kafka-cluster", LITERAL), principal, op, ALLOW)
}

/// Metadata at `version` for `names` (None: all) with the two flags as given, as `who`.
async fn ask(
    rig: &Rig,
    who: &Caller,
    version: i16,
    names: Option<&[&str]>,
    (topics, cluster): (bool, bool),
) -> MetadataResponse {
    let mut req = MetadataRequest::default();
    req.include_topic_authorized_operations = topics;
    req.include_cluster_authorized_operations = cluster;
    req.topics = names.map(|ns| {
        ns.iter()
            .map(|n| {
                let mut t = MetadataRequestTopic::default();
                t.name = Some(wire_name(n));
                t
            })
            .collect()
    });
    let Response::Metadata(r) = rig.ask_as(who, version, Request::Metadata(req)).await else {
        panic!("not Metadata")
    };
    r
}

fn ops(r: &MetadataResponse) -> Vec<(String, i16, i32)> {
    r.topics
        .iter()
        .map(|t| {
            (
                t.name.as_ref().map_or(String::new(), |n| n.0.to_string()),
                t.error_code,
                t.topic_authorized_operations,
            )
        })
        .collect()
}

#[tokio::test]
async fn a_topics_operations_are_the_ones_the_caller_may_do_with_the_implied_describe() {
    let rig = rig_with_topics(&["t"]).await;
    grant(
        &rig,
        vec![
            allow("t", "User:reader", READ),
            allow("t", "User:owner", ALL),
            allow("t", "User:careful", ALL),
            deny("t", "User:careful", DELETE),
            allow("t", "User:configurer", ALTER_CONFIGS),
            allow("t", "User:writer", WRITE),
            deny("t", "User:writer", DESCRIBE),
        ],
    )
    .await;
    for version in 8..=12 {
        let expect = |who: &'static str, entry: (&'static str, i16, i32)| {
            let (rig, who) = (&rig, user(who));
            async move {
                let r = ask(rig, &who, version, Some(&["t"]), (true, false)).await;
                assert_eq!(
                    ops(&r),
                    [(entry.0.to_owned(), entry.1, entry.2)],
                    "v{version} {who:?}"
                );
            }
        };
        // READ grants itself and DESCRIBE.
        expect("reader", ("t", 0, bit(3) | bit(8))).await;
        // ALL grants every operation there is, and no bit is ALL's own.
        expect("owner", ("t", 0, ALL_TOPIC)).await;
        // A DENY of one operation takes that one away and no other.
        expect("careful", ("t", 0, ALL_TOPIC & !bit(6))).await;
        // ALTER_CONFIGS grants DESCRIBE_CONFIGS but not DESCRIBE: the topic is hidden (29, name kept) and the bits
        // are still told, by name.
        expect("configurer", ("t", 29, bit(11) | bit(10))).await;
        // WRITE allows WRITE; DESCRIBE is denied outright (and WRITE's implication does not override a DENY).
        expect("writer", ("t", 29, bit(4))).await;
        // Nothing: the entry is refused and has no operations.
        expect("stranger", ("t", 29, 0)).await;
    }
}

#[tokio::test]
async fn a_field_not_asked_for_or_not_in_the_version_is_the_schema_default() {
    let rig = rig_with_topics(&["t"]).await;
    grant(
        &rig,
        vec![
            allow("t", "User:owner", ALL),
            cluster_acl("User:owner", ALL),
        ],
    )
    .await;
    let owner = user("owner");
    let r = ask(&rig, &owner, 9, Some(&["t"]), (false, false)).await;
    assert_eq!(ops(&r)[0].2, NOT_ASKED);
    assert_eq!(r.cluster_authorized_operations, NOT_ASKED);
    // Asked for the topics only: the cluster's stays unasked.
    let r = ask(&rig, &owner, 9, Some(&["t"]), (true, false)).await;
    assert_eq!(
        (ops(&r)[0].2, r.cluster_authorized_operations),
        (ALL_TOPIC, NOT_ASKED)
    );
    // Before v8 there are no such fields.
    let r = ask(&rig, &owner, 7, Some(&["t"]), (true, true)).await;
    assert_eq!(
        (ops(&r)[0].2, r.cluster_authorized_operations),
        (NOT_ASKED, NOT_ASKED)
    );
}

#[tokio::test]
async fn the_clusters_operations_are_told_in_v8_to_v10_and_are_0_without_describe() {
    let rig = rig_with_topics(&["t"]).await;
    grant(
        &rig,
        vec![
            cluster_acl("User:viewer", DESCRIBE),
            cluster_acl("User:viewer", CREATE),
            cluster_acl("User:admin2", ALL),
            cluster_acl("User:altering", ALTER),
            cluster_acl("User:creator", CREATE),
        ],
    )
    .await;
    let cluster = |who: &'static str, version: i16| {
        let (rig, who) = (&rig, user(who));
        async move {
            ask(rig, &who, version, Some(&["t"]), (false, true))
                .await
                .cluster_authorized_operations
        }
    };
    for version in 8..=10 {
        assert_eq!(
            cluster("viewer", version).await,
            bit(8) | bit(5),
            "v{version}"
        );
        assert_eq!(cluster("admin2", version).await, ALL_CLUSTER, "v{version}");
        // ALTER implies DESCRIBE (so the cluster may be described) and is a bit of its own.
        assert_eq!(
            cluster("altering", version).await,
            bit(7) | bit(8),
            "v{version}"
        );
        // CREATE without DESCRIBE: told nothing (0), not even its own bit.
        assert_eq!(cluster("creator", version).await, 0, "v{version}");
        assert_eq!(cluster("stranger", version).await, 0, "v{version}");
    }
    // v11 and later do not carry it.
    assert_eq!(cluster("admin2", 11).await, NOT_ASKED);
    assert_eq!(cluster("admin2", 12).await, NOT_ASKED);
}

#[tokio::test]
async fn a_list_of_all_topics_has_bits_for_the_topics_it_shows_only() {
    let rig = rig_with_topics(&["a", "b"]).await;
    grant(
        &rig,
        vec![
            allow("a", "User:alice", WRITE),
            allow("b", "User:bob", READ),
        ],
    )
    .await;
    let r = ask(&rig, &user("alice"), 12, None, (true, false)).await;
    assert_eq!(ops(&r), [("a".into(), 0, bit(4) | bit(8))]);
}

#[tokio::test]
async fn super_users_and_an_open_broker_have_every_bit() {
    let rig = rig_with_topics(&["t"]).await;
    grant(
        &rig,
        vec![deny("t", "User:admin", ALL), cluster_acl("User:admin", ALL)],
    )
    .await;
    let r = ask(&rig, &support::acls::admin(), 9, Some(&["t"]), (true, true)).await;
    assert_eq!(
        (ops(&r)[0].2, r.cluster_authorized_operations),
        (ALL_TOPIC, ALL_CLUSTER)
    );
    // A broker with no ACLs and the default allowing: every operation is allowed, so every bit is set.
    let open = rig_with(|_| {});
    open.create_topics_as(
        &support::acls::admin(),
        7,
        vec![support::topic("t", 1)],
        false,
    )
    .await;
    let r = ask(&open, &support::caller(), 9, Some(&["t"]), (true, true)).await;
    assert_eq!(
        (ops(&r)[0].2, r.cluster_authorized_operations),
        (ALL_TOPIC, ALL_CLUSTER)
    );
    let _ = (CREATE, ALTER);
}
