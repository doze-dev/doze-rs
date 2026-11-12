//! Authorization of the transaction APIs: InitProducerId (the transactional half), AddPartitionsToTxn,
//! AddOffsetsToTxn, TxnOffsetCommit, EndTxn, DescribeTransactions, ListTransactions. No row is recorded: each test
//! cites the Kafka 4.3.1 source it is written from (`core/src/main/scala/kafka/server/KafkaApis.scala`, "KA" below).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
#[macro_use]
#[path = "support/authz_groups.rs"]
mod gx;
#[path = "support/authz_calls.rs"]
mod calls;
#[path = "support/authz_txn.rs"]
mod tx;

use gx::{
    DESCRIBE, GROUP, GROUP_AUTH, READ, TOPIC, TOPIC_AUTH, TXN_AUTH, TXN_ID, WRITE, allow, deny,
    make_topic, need, world,
};
use support::acls::{admin, grant, user};
use tx::STRANGER;

const OPERATION_NOT_ATTEMPTED: i16 = 55;
const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;
const TWO_PHASE_COMMIT: i8 = 15;

// ---- InitProducerId, the half with a transactional id (KA:1603-1614)

#[tokio::test]
async fn init_producer_id_with_a_transactional_id_needs_write_on_it() {
    five_ways!(&[need(TXN_ID, "tid", WRITE, TXN_AUTH)], |rig, who| {
        tx::init_txn(rig, who, "tid").await.err().unwrap_or(0)
    });
}

#[tokio::test]
async fn a_two_phase_commit_needs_that_right_on_the_id_as_well() {
    // KA:1610-1614. (The field is a version 6 one, which the broker does not serve; the handler is asked as if.)
    use kafka_wire::api::{Request, Response};
    let rig = world();
    grant(
        &rig,
        vec![
            allow(TXN_ID, "tid", "User:alice", WRITE),
            allow(TXN_ID, "tid", "User:bob", WRITE),
            gx::creation(
                (TXN_ID, "tid", gx::LITERAL),
                "User:bob",
                TWO_PHASE_COMMIT,
                gx::ALLOW,
            ),
        ],
    )
    .await;
    let code = |who: &str| {
        let (rig, who) = (&rig, user(who));
        async move {
            let mut req = kafka_wire::messages::InitProducerIdRequest::default();
            req.transactional_id = Some(tx::txn_id("tid"));
            req.transaction_timeout_ms = 60_000;
            req.enable_2_pc = true;
            let Response::InitProducerId(r) =
                rig.ask_as(&who, 4, Request::InitProducerId(req)).await
            else {
                panic!("not InitProducerId")
            };
            r.error_code
        }
    };
    assert_eq!(code("alice").await, 53);
    assert_eq!(code("bob").await, 0);
}

// ---- AddPartitionsToTxn (KA:1856-1966: WRITE on the id, then WRITE on each topic, before the topic is looked for)

#[tokio::test]
async fn add_partitions_needs_write_on_the_id_and_on_the_topic() {
    five_ways!(
        &[
            need(TXN_ID, "tid", WRITE, TXN_AUTH),
            need(TOPIC, "tpc", WRITE, TOPIC_AUTH)
        ],
        |rig, who| {
            make_topic(rig, "tpc", 1).await;
            let r = tx::add_partitions(rig, who, "tid", STRANGER, &[("tpc", &[0])]).await;
            tx::partition_codes(&r)[0].2
        }
    );
}

#[tokio::test]
async fn a_refused_id_fails_every_partition_of_every_topic_even_one_that_is_not_there() {
    // errorResponseForTransaction (KA:1900-1901): the id's code on every partition asked.
    let rig = world();
    make_topic(&rig, "t", 2).await;
    grant(&rig, vec![allow(TOPIC, "t", "User:bob", WRITE)]).await;
    let r = tx::add_partitions(
        &rig,
        &user("bob"),
        "tid",
        STRANGER,
        &[("t", &[0, 1]), ("nowhere", &[0])],
    )
    .await;
    assert_eq!(
        tx::partition_codes(&r),
        [
            ("t".to_owned(), 0, 53),
            ("t".to_owned(), 1, 53),
            ("nowhere".to_owned(), 0, 53)
        ]
    );
}

#[tokio::test]
async fn one_refused_topic_leaves_the_others_not_attempted() {
    // KA:1925-1935: the topic's partitions get 29, an unknown one 3, and every other partition OPERATION_NOT_ATTEMPTED.
    let rig = world();
    make_topic(&rig, "t", 1).await;
    make_topic(&rig, "u", 2).await;
    grant(
        &rig,
        vec![
            allow(TXN_ID, "tid", "User:alice", WRITE),
            allow(TOPIC, "t", "User:alice", WRITE),
            allow(TOPIC, "ghost", "User:alice", WRITE),
        ],
    )
    .await;
    let r = tx::add_partitions(
        &rig,
        &user("alice"),
        "tid",
        STRANGER,
        &[("t", &[0]), ("u", &[0, 1]), ("ghost", &[0])],
    )
    .await;
    assert_eq!(
        tx::partition_codes(&r),
        [
            ("t".to_owned(), 0, OPERATION_NOT_ATTEMPTED),
            ("u".to_owned(), 0, TOPIC_AUTH),
            ("u".to_owned(), 1, TOPIC_AUTH),
            ("ghost".to_owned(), 0, UNKNOWN_TOPIC_OR_PARTITION)
        ]
    );
}

#[tokio::test]
async fn a_topic_that_is_not_there_is_not_revealed_to_one_who_may_not_write_it() {
    let rig = world();
    grant(
        &rig,
        vec![
            allow(TXN_ID, "tid", "User:alice", WRITE),
            allow(TXN_ID, "tid", "User:bob", WRITE),
            allow(TOPIC, "ghost", "User:bob", WRITE),
        ],
    )
    .await;
    let code = |who: &str| {
        let (rig, who) = (&rig, user(who));
        async move {
            tx::partition_codes(
                &tx::add_partitions(rig, &who, "tid", STRANGER, &[("ghost", &[0])]).await,
            )[0]
            .2
        }
    };
    assert_eq!(code("alice").await, 29);
    assert_eq!(code("bob").await, 3);
}

#[tokio::test]
async fn the_id_is_judged_before_the_topics() {
    let rig = world();
    make_topic(&rig, "t", 1).await;
    grant(&rig, vec![allow(TOPIC, "t", "User:alice", WRITE)]).await;
    let r = tx::add_partitions(&rig, &user("alice"), "tid", STRANGER, &[("t", &[0])]).await;
    assert_eq!(tx::partition_codes(&r)[0].2, 53);
}

// ---- AddOffsetsToTxn (KA:1968-2016: WRITE on the id, then READ on the group)

#[tokio::test]
async fn add_offsets_needs_write_on_the_id_and_read_on_the_group() {
    five_ways!(
        &[
            need(TXN_ID, "tid", WRITE, TXN_AUTH),
            need(GROUP, "grp", READ, GROUP_AUTH)
        ],
        |rig, who| tx::add_offsets(rig, who, "tid", STRANGER, "grp").await
    );
}

#[tokio::test]
async fn add_offsets_judges_the_id_before_the_group() {
    let rig = world();
    assert_eq!(
        tx::add_offsets(&rig, &user("bob"), "tid", STRANGER, "grp").await,
        53
    );
    grant(&rig, vec![allow(TXN_ID, "tid", "User:bob", WRITE)]).await;
    assert_eq!(
        tx::add_offsets(&rig, &user("bob"), "tid", STRANGER, "grp").await,
        30
    );
}

// ---- EndTxn (KA:1668-1709: WRITE on the id, before the coordinator)

#[tokio::test]
async fn end_txn_needs_write_on_the_id() {
    for version in [0_i16, 3] {
        five_ways!(&[need(TXN_ID, "tid", WRITE, TXN_AUTH)], |rig, who| {
            tx::end_txn(rig, who, version, "tid", STRANGER, true).await
        });
    }
}

#[tokio::test]
async fn end_txn_authorizes_the_id_as_sent_before_it_is_judged() {
    // An empty id is INVALID_REQUEST (42) from the coordinator; to a caller that may not write "" it is 53. (No ACL can
    // name an empty id, so only a super user, or the default, passes.)
    let rig = world();
    assert_eq!(
        tx::end_txn(&rig, &user("bob"), 3, "", STRANGER, true).await,
        53
    );
    assert_eq!(tx::end_txn(&rig, &admin(), 3, "", STRANGER, true).await, 42);
    let open = support::rig_with(gx::open);
    assert_eq!(
        tx::end_txn(&open, &user("bob"), 3, "", STRANGER, true).await,
        42
    );
}

// ---- DescribeTransactions and ListTransactions (KA:2540-2593)

#[tokio::test]
async fn describe_transactions_needs_describe_on_the_id() {
    five_ways!(&[need(TXN_ID, "tid", DESCRIBE, TXN_AUTH)], |rig, who| {
        tx::describe_txns(rig, who, &["tid"]).await[0].error_code
    });
}

/// `tid` of the super user with partitions of `t` and `u` added, and `other` with none.
async fn open_transactions(rig: &support::Rig) {
    make_topic(rig, "t", 1).await;
    make_topic(rig, "u", 1).await;
    for id in ["tid", "other"] {
        let producer = tx::init_txn(rig, &admin(), id).await.unwrap();
        if id == "tid" {
            let r =
                tx::add_partitions(rig, &admin(), id, producer, &[("t", &[0]), ("u", &[0])]).await;
            assert!(tx::partition_codes(&r).iter().all(|c| c.2 == 0));
        }
    }
}

#[tokio::test]
async fn describe_transactions_shows_a_refused_id_as_an_entry_of_53_and_only_the_topics_described()
{
    let rig = world();
    open_transactions(&rig).await;
    grant(
        &rig,
        vec![
            allow(TXN_ID, "tid", "User:alice", DESCRIBE),
            allow(TOPIC, "t", "User:alice", DESCRIBE),
        ],
    )
    .await;
    let seen = tx::describe_txns(&rig, &user("alice"), &["other", "tid"]).await;
    assert_eq!(
        (seen[0].transactional_id.0.as_str(), seen[0].error_code),
        ("other", 53)
    );
    assert!(seen[0].topics.is_empty());
    assert_eq!(
        (seen[0].transaction_state.as_str(), seen[0].producer_id.0),
        ("", 0)
    );
    // The topics the caller may not describe are left out of a visible transaction (KA:2552-2559).
    assert_eq!(seen[1].error_code, 0);
    let topics: Vec<_> = seen[1]
        .topics
        .iter()
        .map(|t| t.topic.0.as_str().to_owned())
        .collect();
    assert_eq!(topics, ["t"]);
    // The super user sees both.
    let seen = tx::describe_txns(&rig, &admin(), &["tid"]).await;
    assert_eq!(seen[0].topics.len(), 2);
}

#[tokio::test]
async fn list_transactions_shows_only_the_ids_the_caller_may_describe() {
    let rig = world();
    open_transactions(&rig).await;
    grant(
        &rig,
        vec![
            allow(TXN_ID, "tid", "User:alice", DESCRIBE),
            allow(TXN_ID, "other", "User:carol", WRITE),
            deny(TXN_ID, "other", "User:carol", DESCRIBE),
            allow(TXN_ID, "*", "User:wild", DESCRIBE),
            gx::creation(
                (TXN_ID, "ot", gx::PREFIXED),
                "User:pre",
                DESCRIBE,
                gx::ALLOW,
            ),
        ],
    )
    .await;
    let seen = |who: &str| {
        let (rig, who) = (&rig, user(who));
        async move { tx::list_txns(rig, &who).await }
    };
    assert_eq!(seen("alice").await, ["tid"]);
    assert!(seen("bob").await.is_empty());
    // WRITE implies DESCRIBE, and a DENY of DESCRIBE takes it away again.
    assert!(seen("carol").await.is_empty());
    assert_eq!(seen("wild").await, ["other", "tid"]);
    assert_eq!(seen("pre").await, ["other"]);
    assert_eq!(seen("admin").await, ["other", "tid"]);
    // Any user.
    let rig = world();
    open_transactions(&rig).await;
    grant(&rig, vec![allow(TXN_ID, "tid", "User:*", DESCRIBE)]).await;
    assert_eq!(tx::list_txns(&rig, &user("zed")).await, ["tid"]);
    // A broker whose default allows lists all until an ACL covers an id.
    let open = support::rig_with(gx::open);
    open_transactions(&open).await;
    assert_eq!(tx::list_txns(&open, &user("zed")).await, ["other", "tid"]);
    grant(&open, vec![allow(TXN_ID, "other", "User:alice", DESCRIBE)]).await;
    assert_eq!(tx::list_txns(&open, &user("zed")).await, ["tid"]);
}
