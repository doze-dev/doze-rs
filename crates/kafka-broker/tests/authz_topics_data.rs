//! Produce, Fetch, ListOffsets, DeleteRecords and DescribeProducers under ACLs (slice 8d-1): the shape of each denial, the
//! order of the answers and what is hidden. Every row is from the Kafka 4.3.1 source, not from a recording:
//! `KafkaApis.scala` `handleProduceRequest` (396-528), `handleFetchRequest` (563-675), `handleListOffsetRequest`
//! (781-821), `handleDeleteRecordsRequest` (1538-1601), `handleDescribeProducersRequest` (2485-2529).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
#[path = "support/authz_topics.rs"]
mod topics;

use std::time::Duration;

use kafka_broker::Outcome;
use kafka_wire::api::Request;
use kafka_wire::messages::BrokerId;
use support::acls::{
    ALLOW, CLUSTER, CREATE, DENY, DESCRIBE, LITERAL, READ, TXN_ID, WRITE, admin, creation, user,
};
use support::{Rig, from, wire_name};
use topics::*;

const ALTER: i8 = 7;
const IDEMPOTENT_WRITE: i8 = 12;
const UNKNOWN_TOPIC_ID: i16 = 100;
const TRANSACTIONAL_ID_AUTHORIZATION_FAILED: i16 = 53;

async fn end_of(rig: &Rig, topic: &str) -> i64 {
    let r = rig.list_offsets_as(&admin(), 7, &[(topic, &[0])]).await;
    r.topics[0].partitions[0].offset
}

// Produce

#[tokio::test]
async fn produce_answers_each_topic_by_its_own_right_and_stores_only_what_was_allowed() {
    for version in [3, 5, 7, 9] {
        let rig = rig_with_topics(&["a", "b"]).await;
        grant(&rig, vec![allow("a", "User:alice", WRITE)]).await;
        let r = rig
            .produce_to_as(&user("alice"), version, -1, &[("b", 0), ("a", 0)])
            .await;
        assert_eq!(
            Rig::produced(&r),
            [("b".into(), 0, 29), ("a".into(), 0, 0)],
            "v{version}: the denied topic's partitions are 29, in the order asked"
        );
        let denied = &r.responses[0].partition_responses[0];
        assert_eq!((denied.base_offset, denied.log_start_offset), (-1, -1));
        assert_eq!(
            end_of(&rig, "a").await,
            2,
            "the allowed topic got its batch"
        );
        assert_eq!(end_of(&rig, "b").await, 1, "the other got nothing");
    }
}

#[tokio::test]
async fn every_partition_of_a_refused_topic_is_refused_and_a_missing_one_is_hidden() {
    let rig = rig_with_topics(&["a"]).await;
    let req = support::produce_request(
        -1,
        "a",
        &[(0, support::records(1)), (7, support::records(1))],
    );
    let r = rig.produce_as(&user("nobody"), 9, req).await;
    assert_eq!(
        Rig::produced(&r),
        [("a".into(), 0, 29), ("a".into(), 7, 29)]
    );
    let ghost = support::produce_request(-1, "ghost", &[(0, support::records(1))]);
    let r = rig.produce_as(&user("nobody"), 9, ghost).await;
    assert_eq!(
        Rig::produced(&r),
        [("ghost".into(), 0, 29)],
        "not 3: nobody may not know"
    );
}

#[tokio::test]
async fn produce_needs_only_topic_write_not_idempotent_write() {
    // KafkaApis.handleProduceRequest checks WRITE on the topic and nothing else; IDEMPOTENT_WRITE is InitProducerId's.
    let rig = rig_with_topics(&["a"]).await;
    grant(
        &rig,
        vec![
            allow("a", "User:alice", WRITE),
            creation(
                (CLUSTER, "kafka-cluster", LITERAL),
                "User:bob",
                IDEMPOTENT_WRITE,
                ALLOW,
            ),
        ],
    )
    .await;
    let idempotent = |seq| from(900, 0, seq, false, &[("k", "v")]);
    let mut req = support::produce_request(-1, "a", &[(0, idempotent(0))]);
    req.transactional_id = None;
    assert_eq!(
        Rig::produced(&rig.produce_as(&user("alice"), 9, req).await)[0].2,
        0
    );
    let req = support::produce_request(-1, "a", &[(0, idempotent(1))]);
    assert_eq!(
        Rig::produced(&rig.produce_as(&user("bob"), 9, req).await)[0].2,
        29,
        "IDEMPOTENT_WRITE on the cluster does not let bob write a topic"
    );
}

fn transactional(id: &str) -> kafka_wire::messages::ProduceRequest {
    let mut req = support::produce_request(-1, "a", &[(0, from(7, 0, 0, true, &[("k", "v")]))]);
    req.topic_data.push(
        support::produce_request(-1, "b", &[(0, from(7, 0, 0, true, &[("k", "v")]))])
            .topic_data
            .remove(0),
    );
    req.transactional_id = Some(kafka_wire::messages::TransactionalId(
        kafka_wire::StrBytes::from_string(id.to_owned()),
    ));
    req
}

#[tokio::test]
async fn a_transactional_batch_needs_write_on_the_transactional_id_before_any_topic_is_looked_at() {
    // handleProduceRequest: the transactional id first (a null id is denied), as a whole-request error on every
    // partition (getErrorResponse), then each topic.
    let rig = rig_with_topics(&["a", "b"]).await;
    let txn = |principal: &str| creation((TXN_ID, "tx", LITERAL), principal, WRITE, ALLOW);
    grant(
        &rig,
        vec![
            allow("a", "User:alice", WRITE),
            allow("b", "User:alice", WRITE),
            allow("a", "User:bob", WRITE),
            txn("User:bob"),
            txn("User:carol"),
        ],
    )
    .await;
    // alice has both topics and no right to the id: 53 on every partition of both topics.
    let r = rig.produce_as(&user("alice"), 9, transactional("tx")).await;
    assert_eq!(
        Rig::produced(&r),
        [("a".into(), 0, 53), ("b".into(), 0, 53)]
    );
    // No id at all is denied the same way.
    let mut req = transactional("tx");
    req.transactional_id = None;
    let r = rig.produce_as(&user("alice"), 9, req).await;
    assert!(
        Rig::produced(&r)
            .iter()
            .all(|p| p.2 == TRANSACTIONAL_ID_AUTHORIZATION_FAILED)
    );
    // carol has the id and no topic: the topics are refused one by one, now.
    let r = rig.produce_as(&user("carol"), 9, transactional("tx")).await;
    assert_eq!(
        Rig::produced(&r),
        [("a".into(), 0, 29), ("b".into(), 0, 29)]
    );
    // bob has the id and topic a: a is looked at (and fails for want of an open transaction, which is not an
    // authorization error), b is 29.
    let r = rig.produce_as(&user("bob"), 9, transactional("tx")).await;
    let got = Rig::produced(&r);
    assert_ne!(got[0].2, 29);
    assert_ne!(got[0].2, 53);
    assert_eq!(got[1], ("b".into(), 0, 29));
}

#[tokio::test]
async fn a_batch_that_is_not_transactional_asks_nothing_of_the_transactional_id() {
    let rig = rig_with_topics(&["a"]).await;
    grant(&rig, vec![allow("a", "User:alice", WRITE)]).await;
    let mut req = support::produce_request(-1, "a", &[(0, support::records(1))]);
    req.transactional_id = Some(kafka_wire::messages::TransactionalId(
        kafka_wire::StrBytes::from_string("tx".to_owned()),
    ));
    assert_eq!(
        Rig::produced(&rig.produce_as(&user("alice"), 9, req).await)[0].2,
        0
    );
}

#[tokio::test]
async fn with_no_acks_a_refusal_closes_the_connection_and_a_success_is_not_answered() {
    // ProduceRequest.getErrorResponse is null for acks 0: Kafka closes the connection (KafkaApis 507-521).
    let rig = rig_with_topics(&["a", "b"]).await;
    grant(&rig, vec![allow("a", "User:alice", WRITE)]).await;
    let ask = |topic: &'static str| {
        let (rig, who) = (&rig, user("alice"));
        async move {
            let req = support::produce_request(0, topic, &[(0, support::records(1))]);
            rig.outcome_as(&who, 9, Request::Produce(req)).await
        }
    };
    assert!(matches!(ask("b").await, Outcome::Close));
    assert!(matches!(ask("a").await, Outcome::NoResponse));
}

// Fetch

fn fetch_of(asks: &[support::Ask<'_>]) -> kafka_wire::messages::FetchRequest {
    support::fetch_request(asks, 0, 1, 1 << 20)
}

#[tokio::test]
async fn fetch_answers_each_partition_by_the_topics_right_in_every_version() {
    let rig = rig_with_topics(&["a", "b"]).await;
    grant(&rig, vec![allow("a", "User:alice", READ)]).await;
    for version in 4..=12 {
        let r = rig
            .fetch_topics_as(
                &user("alice"),
                version,
                &[("b", 0, 0, 1 << 20), ("a", 0, 0, 1 << 20)],
            )
            .await;
        assert_eq!(
            Rig::fetched(&r),
            [("b".into(), 0, 29), ("a".into(), 0, 0)],
            "v{version}"
        );
        let data = &r.responses[1].partitions[0];
        assert!(
            data.records.as_ref().is_some_and(|b| !b.is_empty()),
            "v{version}: a is served"
        );
        let denied = &r.responses[0].partitions[0];
        assert!(
            denied.records.as_ref().is_none_or(|b| b.is_empty()),
            "nothing of b leaves"
        );
    }
    let r = rig
        .fetch_topics_as(&user("alice"), 12, &[("ghost", 0, 0, 1 << 20)])
        .await;
    assert_eq!(Rig::fetched(&r), [("ghost".into(), 0, 29)]);
}

#[tokio::test]
async fn a_refused_fetch_is_answered_at_once_and_does_not_wait_for_data() {
    let rig = rig_with_topics(&["a"]).await;
    let mut req = support::fetch_request(&[("a", 0, 5, 1 << 20)], 20_000, 1, 1 << 20);
    req.max_wait_ms = 20_000;
    let nobody = user("nobody");
    let asked = rig.ask_as(&nobody, 12, Request::Fetch(req));
    let r = tokio::time::timeout(Duration::from_secs(5), asked)
        .await
        .expect("it waited");
    let kafka_wire::api::Response::Fetch(r) = r else {
        panic!("not Fetch")
    };
    assert_eq!(Rig::fetched(&r), [("a".into(), 0, 29)]);
}

#[tokio::test]
async fn a_follower_fetch_needs_cluster_action_and_no_topic_right() {
    // handleFetchRequest: replicaId >= 0 is a follower: CLUSTER_ACTION on the cluster, once; without it every partition
    // is TOPIC_AUTHORIZATION_FAILED, whatever READ rights the caller has.
    let rig = rig_with_topics(&["a"]).await;
    grant(
        &rig,
        vec![
            allow("a", "User:reader", READ),
            creation(
                (CLUSTER, "kafka-cluster", LITERAL),
                "User:broker",
                CLUSTER_ACTION,
                ALLOW,
            ),
        ],
    )
    .await;
    let follower = |who: &str| {
        let (rig, who) = (&rig, user(who));
        async move {
            let mut req = fetch_of(&[("a", 0, 0, 1 << 20)]);
            req.replica_id = BrokerId(0);
            let r = rig.ask_as(&who, 12, Request::Fetch(req)).await;
            let kafka_wire::api::Response::Fetch(r) = r else {
                panic!("not Fetch")
            };
            Rig::fetched(&r)
        }
    };
    assert_eq!(follower("reader").await, [("a".into(), 0, 29)]);
    assert_eq!(follower("broker").await, [("a".into(), 0, 0)]);
    // And the same caller as a consumer (replica -1) is judged by READ on the topic.
    let r = rig
        .fetch_topics_as(&user("broker"), 12, &[("a", 0, 0, 1 << 20)])
        .await;
    assert_eq!(Rig::fetched(&r), [("a".into(), 0, 29)]);
}

#[tokio::test]
async fn fetch_by_topic_id_judges_the_name_the_id_stands_for_and_an_unknown_id_is_unknown_to_all() {
    let rig = rig_with_topics(&["a"]).await;
    grant(&rig, vec![allow("a", "User:alice", READ)]).await;
    let id = rig.id_of("a").await;
    let by_id = |who: &str, id: kafka_wire::Uuid| {
        let (rig, who) = (&rig, user(who));
        async move {
            let mut req = fetch_of(&[("a", 0, 0, 1 << 20)]);
            req.topics[0].topic = wire_name("");
            req.topics[0].topic_id = id;
            let r = rig.ask_as(&who, 13, Request::Fetch(req)).await;
            let kafka_wire::api::Response::Fetch(r) = r else {
                panic!("not Fetch")
            };
            r.responses[0].partitions[0].error_code
        }
    };
    assert_eq!(by_id("alice", id).await, 0);
    assert_eq!(by_id("bob", id).await, 29);
    let unknown = kafka_wire::Uuid::from_u128(0xdead_beef);
    assert_eq!(
        by_id("bob", unknown).await,
        UNKNOWN_TOPIC_ID,
        "(KA:589-592: before authorization)"
    );
}

// ListOffsets

#[tokio::test]
async fn list_offsets_puts_the_refused_topics_after_the_others_with_no_offset() {
    let rig = rig_with_topics(&["a", "b"]).await;
    grant(&rig, vec![allow("a", "User:alice", DESCRIBE)]).await;
    for version in 1..=7 {
        let r = rig
            .list_offsets_as(&user("alice"), version, &[("b", &[0, 1]), ("a", &[0])])
            .await;
        let shape: Vec<_> = r
            .topics
            .iter()
            .map(|t| {
                (
                    t.name.0.to_string(),
                    t.partitions
                        .iter()
                        .map(|p| p.error_code)
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        assert_eq!(
            shape,
            [("a".into(), vec![0]), ("b".into(), vec![29, 29])],
            "v{version}: the allowed topic first (KA:805-807)"
        );
        let denied = &r.topics[1].partitions[0];
        assert_eq!((denied.timestamp, denied.offset), (-1, -1));
        assert_eq!(r.topics[0].partitions[0].offset, 1, "a is answered in full");
    }
}

#[tokio::test]
async fn list_offsets_is_granted_by_any_right_that_implies_describe_and_denied_only_by_describe() {
    for (op, answer) in [
        (READ, 0),
        (WRITE, 0),
        (DELETE, 0),
        (ALTER, 0),
        (DESCRIBE, 0),
        (CREATE, 29),
        (ALTER_CONFIGS, 29),
    ] {
        let rig = rig_with_topics(&["a"]).await;
        grant(&rig, vec![allow("a", "User:alice", op)]).await;
        let r = rig.list_offsets_as(&user("alice"), 7, &[("a", &[0])]).await;
        assert_eq!(r.topics[0].partitions[0].error_code, answer, "ALLOW {op}");
    }
    let rig = rig_with_topics(&["a"]).await;
    grant(
        &rig,
        vec![
            allow("a", "User:alice", ALTER),
            deny("a", "User:alice", READ),
        ],
    )
    .await;
    let r = rig.list_offsets_as(&user("alice"), 7, &[("a", &[0])]).await;
    assert_eq!(
        r.topics[0].partitions[0].error_code, 0,
        "DENY READ is not DENY DESCRIBE"
    );
    grant(&rig, vec![deny("a", "User:alice", DESCRIBE)]).await;
    let r = rig.list_offsets_as(&user("alice"), 7, &[("a", &[0])]).await;
    assert_eq!(r.topics[0].partitions[0].error_code, 29);
}

// DeleteRecords

#[tokio::test]
async fn delete_records_refuses_a_topic_per_partition_with_no_low_watermark_and_deletes_nothing() {
    for version in 0..=2 {
        let rig = rig_with_topics(&["a", "b"]).await;
        grant(&rig, vec![allow("a", "User:alice", DELETE)]).await;
        let r = rig
            .delete_records_as(&user("alice"), version, &[("b", &[0, 1]), ("a", &[0])])
            .await;
        let shape: Vec<_> = r
            .topics
            .iter()
            .map(|t| {
                (
                    t.name.0.to_string(),
                    t.partitions
                        .iter()
                        .map(|p| (p.error_code, p.low_watermark))
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        assert_eq!(
            shape,
            [
                ("b".into(), vec![(29, -1), (29, -1)]),
                ("a".into(), vec![(0, 1)])
            ],
            "v{version}: refused where it is, in the order asked (Kafka's is a map's)"
        );
        // b's record is still there to be read from offset 0; a's was deleted, so its start moved up to 1.
        let still = rig
            .fetch_topics_as(&admin(), 12, &[("b", 0, 0, 1 << 20), ("a", 0, 0, 1 << 20)])
            .await;
        assert_eq!(
            Rig::fetched(&still),
            [("b".into(), 0, 0), ("a".into(), 0, 1)],
            "1 is OFFSET_OUT_OF_RANGE"
        );
    }
}

// DescribeProducers

#[tokio::test]
async fn describe_producers_refuses_each_partition_with_the_error_text_and_judges_the_name_first() {
    let rig = rig_with_topics(&["a"]).await;
    grant(&rig, vec![allow("a", "User:alice", READ)]).await;
    let r = rig
        .describe_producers_as(&user("bob"), &[("a", &[0, 1]), ("ghost", &[0])])
        .await;
    for t in &r.topics {
        for p in &t.partitions {
            assert_eq!(
                p.error_code, 29,
                "{}: bob may not read it, whether it is there or not",
                t.name.0
            );
            assert_eq!(
                p.error_message.as_deref(),
                Some("Topic authorization failed.")
            );
        }
    }
    // A name that cannot be a topic's is INVALID_TOPIC_EXCEPTION, before READ is asked (KA:2503-2505).
    let r = rig
        .describe_producers_as(&user("bob"), &[("bad name", &[0])])
        .await;
    assert_eq!(r.topics[0].partitions[0].error_code, 17);
    // alice may read "a": its producers are told (none yet), and a partition it does not have is unknown.
    let r = rig
        .describe_producers_as(&user("alice"), &[("a", &[0, 5])])
        .await;
    assert_eq!(
        r.topics[0]
            .partitions
            .iter()
            .map(|p| p.error_code)
            .collect::<Vec<_>>(),
        [0, 3]
    );
}
