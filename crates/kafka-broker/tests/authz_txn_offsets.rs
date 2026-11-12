//! Authorization of TxnOffsetCommit, and a whole transaction run with and without the rights. No row is recorded: each test
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
    GROUP, GROUP_AUTH, READ, TOPIC, TOPIC_AUTH, TXN_AUTH, TXN_ID, WRITE, allow, deny, make_topic,
    need, world,
};
use support::acls::{admin, grant, user};
use tx::STRANGER;

const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;

// ---- TxnOffsetCommit (KA:2018-2116: WRITE on the id, READ on the group, READ on each topic)

#[tokio::test]
async fn txn_offset_commit_needs_write_on_the_id_read_on_the_group_and_read_on_the_topic() {
    five_ways!(
        &[
            need(TXN_ID, "tid", WRITE, TXN_AUTH),
            need(GROUP, "grp", READ, GROUP_AUTH),
            need(TOPIC, "tpc", READ, TOPIC_AUTH)
        ],
        |rig, who| {
            make_topic(rig, "tpc", 1).await;
            let r = tx::txn_offset_commit(rig, who, "tid", STRANGER, "grp", &[("tpc", &[0])]).await;
            tx::offset_codes(&r)[0].2
        }
    );
}

#[tokio::test]
async fn a_refused_id_or_group_fails_every_partition_asked_whether_or_not_it_is_there() {
    // getErrorResponse (KA:2046-2051): the id's 53, else the group's 30, on every partition of every topic.
    let rig = world();
    make_topic(&rig, "t", 2).await;
    let asked: &[(&str, &[i32])] = &[("t", &[0, 1]), ("ghost", &[0])];
    let r = tx::txn_offset_commit(&rig, &user("bob"), "tid", STRANGER, "grp", asked).await;
    let codes: Vec<i16> = tx::offset_codes(&r).iter().map(|c| c.2).collect();
    assert_eq!(codes, [53, 53, 53]);
    grant(&rig, vec![allow(TXN_ID, "tid", "User:bob", WRITE)]).await;
    let r = tx::txn_offset_commit(&rig, &user("bob"), "tid", STRANGER, "grp", asked).await;
    let codes: Vec<i16> = tx::offset_codes(&r).iter().map(|c| c.2).collect();
    assert_eq!(codes, [30, 30, 30]);
}

#[tokio::test]
async fn offsets_of_a_refused_topic_are_not_staged_with_the_transaction() {
    let rig = world();
    make_topic(&rig, "t", 1).await;
    make_topic(&rig, "u", 1).await;
    grant(
        &rig,
        vec![
            allow(TXN_ID, "tid", "User:alice", WRITE),
            allow(GROUP, "grp", "User:alice", READ),
            allow(TOPIC, "t", "User:alice", READ),
        ],
    )
    .await;
    let alice = user("alice");
    let producer = tx::init_txn(&rig, &alice, "tid").await.unwrap();
    assert_eq!(
        tx::add_offsets(&rig, &alice, "tid", producer, "grp").await,
        0
    );
    let staged = tx::txn_offset_commit(
        &rig,
        &alice,
        "tid",
        producer,
        "grp",
        &[("t", &[0]), ("u", &[0])],
    )
    .await;
    let codes: Vec<i16> = tx::offset_codes(&staged).iter().map(|c| c.2).collect();
    assert_eq!(codes, [0, 29]);
    assert_eq!(tx::end_txn(&rig, &alice, 3, "tid", producer, true).await, 0);
    // The commit made t's offset the group's; u's was never staged.
    let got = calls::fetch(&rig, &admin(), 7, "grp", None).await;
    let names: Vec<String> = got.topics.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(names, ["t"]);
}

#[tokio::test]
async fn a_refused_topic_is_29_on_its_partitions_and_hidden_existence_holds() {
    let rig = world();
    make_topic(&rig, "t", 1).await;
    make_topic(&rig, "u", 1).await;
    grant(
        &rig,
        vec![
            allow(TXN_ID, "tid", "User:alice", WRITE),
            allow(GROUP, "grp", "User:alice", READ),
            allow(TOPIC, "t", "User:alice", READ),
            allow(TOPIC, "ghost", "User:alice", READ),
            deny(TOPIC, "u", "User:alice", READ),
        ],
    )
    .await;
    let asked: &[(&str, &[i32])] = &[("u", &[0]), ("t", &[0]), ("ghost", &[0]), ("hidden", &[0])];
    let r = tx::txn_offset_commit(&rig, &user("alice"), "tid", STRANGER, "grp", asked).await;
    let seen = tx::offset_codes(&r);
    let code = |topic: &str| seen.iter().find(|c| c.0 == topic).unwrap().2;
    assert_eq!(code("u"), 29);
    // Not there, and the caller may read it: unknown (3). Not there, and may not: 29 (the name is authorized first).
    assert_eq!(code("ghost"), UNKNOWN_TOPIC_OR_PARTITION);
    assert_eq!(code("hidden"), 29);
    // t is judged by the coordinator, which knows no producer 1: not a refusal of ours.
    assert_ne!(code("t"), 29);
}

#[tokio::test]
async fn a_producer_runs_a_whole_transaction_with_the_rights_and_is_refused_at_each_step_without() {
    let rig = world();
    make_topic(&rig, "t", 1).await;
    grant(
        &rig,
        vec![
            allow(TXN_ID, "tid", "User:alice", WRITE),
            allow(TOPIC, "t", "User:alice", WRITE),
            allow(TOPIC, "t", "User:alice", READ),
            allow(GROUP, "grp", "User:alice", READ),
        ],
    )
    .await;
    let alice = user("alice");
    let producer = tx::init_txn(&rig, &alice, "tid").await.unwrap();
    let added = tx::add_partitions(&rig, &alice, "tid", producer, &[("t", &[0])]).await;
    assert_eq!(tx::partition_codes(&added)[0].2, 0);
    assert_eq!(
        tx::add_offsets(&rig, &alice, "tid", producer, "grp").await,
        0
    );
    let staged = tx::txn_offset_commit(&rig, &alice, "tid", producer, "grp", &[("t", &[0])]).await;
    assert_eq!(tx::offset_codes(&staged)[0].2, 0);
    assert_eq!(tx::end_txn(&rig, &alice, 3, "tid", producer, true).await, 0);
    // The same steps by one who holds nothing, with the producer alice was given.
    let bob = user("bob");
    assert_eq!(tx::init_txn(&rig, &bob, "tid").await, Err(53));
    let added = tx::add_partitions(&rig, &bob, "tid", producer, &[("t", &[0])]).await;
    assert_eq!(tx::partition_codes(&added)[0].2, 53);
    assert_eq!(
        tx::add_offsets(&rig, &bob, "tid", producer, "grp").await,
        53
    );
    let staged = tx::txn_offset_commit(&rig, &bob, "tid", producer, "grp", &[("t", &[0])]).await;
    assert_eq!(tx::offset_codes(&staged)[0].2, 53);
    assert_eq!(tx::end_txn(&rig, &bob, 3, "tid", producer, true).await, 53);
}
