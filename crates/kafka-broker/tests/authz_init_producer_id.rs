//! InitProducerId under ACLs (kafka-semantics 2.1 and 1.5, KafkaApis.scala 1603-1666), and the recorded
//! `idempotent-write-from-topic-write`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use support::acls::{
    ALL, ALLOW, ALTER, CLUSTER, DENY, IDEMPOTENT_WRITE, LITERAL, PREFIXED, READ, TOPIC, TXN_ID,
    WRITE, closed_world, creation, grant, user,
};
use support::{Rig, rig_with};

const CLUSTER_AUTHORIZATION_FAILED: i16 = 31;
const TRANSACTIONAL_ID_AUTHORIZATION_FAILED: i16 = 53;

/// The error code of an InitProducerId with no transactional id.
async fn idempotent(rig: &Rig, who: &str) -> i16 {
    let (code, producer) = rig.init_producer_id_as(&user(who), None).await;
    assert_eq!(
        code == 0,
        producer >= 0,
        "a producer id comes with success only"
    );
    code
}

#[tokio::test]
async fn the_recorded_case_write_on_a_topic_is_enough_and_nothing_is_not() {
    let rig = rig_with(closed_world);
    grant(
        &rig,
        vec![creation((TOPIC, "iw", LITERAL), "User:alice", WRITE, ALLOW)],
    )
    .await;
    assert_eq!(idempotent(&rig, "alice").await, 0);
    assert_eq!(idempotent(&rig, "bob").await, CLUSTER_AUTHORIZATION_FAILED);
    // The refusal is the whole answer: no producer id, epoch -1.
    let (code, producer) = rig.init_producer_id_as(&user("bob"), None).await;
    assert_eq!((code, producer), (31, -1));
    assert_eq!(idempotent(&rig, "admin").await, 0, "a super user passes");
}

#[tokio::test]
async fn idempotent_write_on_the_cluster_is_enough_too() {
    let rig = rig_with(closed_world);
    grant(
        &rig,
        vec![
            creation(
                (CLUSTER, "kafka-cluster", LITERAL),
                "User:carol",
                IDEMPOTENT_WRITE,
                ALLOW,
            ),
            // ALTER on the cluster implies nothing of the kind.
            creation(
                (CLUSTER, "kafka-cluster", LITERAL),
                "User:dave",
                ALTER,
                ALLOW,
            ),
            // READ on a topic is not WRITE.
            creation((TOPIC, "t", LITERAL), "User:erin", READ, ALLOW),
        ],
    )
    .await;
    assert_eq!(idempotent(&rig, "carol").await, 0);
    assert_eq!(idempotent(&rig, "dave").await, 31);
    assert_eq!(idempotent(&rig, "erin").await, 31);
}

#[tokio::test]
async fn write_on_any_topic_by_name_prefix_wildcard_or_any_user() {
    let rig = rig_with(closed_world);
    grant(
        &rig,
        vec![
            creation((TOPIC, "orders-", PREFIXED), "User:pre", WRITE, ALLOW),
            creation((TOPIC, "*", LITERAL), "User:wild", WRITE, ALLOW),
            creation((TOPIC, "shared", LITERAL), "User:*", WRITE, ALLOW),
            creation((TOPIC, "t", LITERAL), "User:all", ALL, ALLOW),
        ],
    )
    .await;
    for who in ["pre", "wild", "anyone", "all"] {
        assert_eq!(idempotent(&rig, who).await, 0, "{who}");
    }
}

#[tokio::test]
async fn a_deny_takes_the_right_away_only_where_it_dominates() {
    let rig = rig_with(closed_world);
    grant(
        &rig,
        vec![
            // Denied topic: its allow is dominated.
            creation((TOPIC, "t", LITERAL), "User:one", WRITE, ALLOW),
            creation((TOPIC, "t", LITERAL), "User:one", WRITE, DENY),
            // A prefix deny dominates the allows under it, but another allow is left.
            creation((TOPIC, "top", LITERAL), "User:two", WRITE, ALLOW),
            creation((TOPIC, "to", PREFIXED), "User:two", WRITE, DENY),
            creation((TOPIC, "other", LITERAL), "User:two", WRITE, ALLOW),
            // A wildcard deny ends it.
            creation((TOPIC, "ok", LITERAL), "User:three", WRITE, ALLOW),
            creation((TOPIC, "*", LITERAL), "User:three", ALL, DENY),
            // A deny for somebody else does nothing.
            creation((TOPIC, "ok", LITERAL), "User:four", WRITE, ALLOW),
            creation((TOPIC, "*", LITERAL), "User:one", ALL, DENY),
        ],
    )
    .await;
    assert_eq!(idempotent(&rig, "one").await, 31);
    assert_eq!(idempotent(&rig, "two").await, 0);
    assert_eq!(idempotent(&rig, "three").await, 31);
    assert_eq!(idempotent(&rig, "four").await, 0);
    // A deny of the cluster's IdempotentWrite does not take away the topic route.
    grant(
        &rig,
        vec![creation(
            (CLUSTER, "kafka-cluster", LITERAL),
            "User:four",
            IDEMPOTENT_WRITE,
            DENY,
        )],
    )
    .await;
    assert_eq!(idempotent(&rig, "four").await, 0);
}

#[tokio::test]
async fn with_the_default_allowing_the_name_hardcode_decides() {
    // allow.everyone.if.no.acl.found is true: nothing covers the cluster, so everyone may.
    let rig = rig_with(|_| {});
    assert_eq!(idempotent(&rig, "anyone").await, 0);
    // Once an ACL covers the cluster, the cluster route is closed to the others ...
    grant_open(
        &rig,
        creation(
            (CLUSTER, "kafka-cluster", LITERAL),
            "User:alice",
            ALTER,
            ALLOW,
        ),
    )
    .await;
    // ... but the topic route asks about a topic called `hardcode` first, which nothing covers, and the default
    // lets everyone through it.
    assert_eq!(idempotent(&rig, "bob").await, 0);
    grant_open(
        &rig,
        creation((TOPIC, "t", LITERAL), "User:alice", WRITE, ALLOW),
    )
    .await;
    assert_eq!(idempotent(&rig, "bob").await, 0);
    // A wildcard covers `hardcode`: the default is off, and bob has no write on any topic.
    grant_open(
        &rig,
        creation((TOPIC, "*", LITERAL), "User:alice", WRITE, ALLOW),
    )
    .await;
    assert_eq!(idempotent(&rig, "bob").await, 31);
    assert_eq!(idempotent(&rig, "alice").await, 0);
}

async fn grant_open(rig: &Rig, acl: kafka_wire::messages::create_acls_request::AclCreation) {
    let r = rig.create_acls_as(&user("alice"), 3, vec![acl]).await;
    assert_eq!(r[0].error_code, 0);
}

#[tokio::test]
async fn a_transactional_id_needs_write_on_that_id() {
    let rig = rig_with(closed_world);
    grant(
        &rig,
        vec![
            creation((TXN_ID, "tx-1", LITERAL), "User:alice", WRITE, ALLOW),
            creation((TXN_ID, "batch-", PREFIXED), "User:alice", WRITE, ALLOW),
            // Write on a topic and idempotent write on the cluster are not the transactional id.
            creation((TOPIC, "t", LITERAL), "User:bob", WRITE, ALLOW),
            creation(
                (CLUSTER, "kafka-cluster", LITERAL),
                "User:bob",
                IDEMPOTENT_WRITE,
                ALLOW,
            ),
            creation((TXN_ID, "tx-1", LITERAL), "User:carol", WRITE, ALLOW),
            creation((TXN_ID, "tx-1", LITERAL), "User:carol", WRITE, DENY),
        ],
    )
    .await;
    let code = |who: &'static str, id: &'static str| {
        let rig = &rig;
        async move { rig.init_producer_id_as(&user(who), Some(id)).await.0 }
    };
    assert_eq!(code("alice", "tx-1").await, 0);
    assert_eq!(code("alice", "batch-7").await, 0);
    assert_eq!(
        code("alice", "tx-2").await,
        TRANSACTIONAL_ID_AUTHORIZATION_FAILED
    );
    assert_eq!(
        code("bob", "tx-1").await,
        TRANSACTIONAL_ID_AUTHORIZATION_FAILED
    );
    assert_eq!(
        code("carol", "tx-1").await,
        TRANSACTIONAL_ID_AUTHORIZATION_FAILED
    );
    assert_eq!(code("admin", "tx-9").await, 0, "a super user passes");
    // The authorization comes before the id is judged: an empty id is 53 for the one who may not, 42 for the one who may.
    assert_eq!(code("bob", "").await, TRANSACTIONAL_ID_AUTHORIZATION_FAILED);
    assert_eq!(code("admin", "").await, 42);
    // The refusal leaves no mark: alice, who may, is not fenced by bob's try.
    assert_eq!(code("alice", "tx-1").await, 0);
}
