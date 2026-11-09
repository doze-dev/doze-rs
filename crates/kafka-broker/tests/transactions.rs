//! Transactions through the broker: a producer that owns a transactional id writes to partitions, commits or aborts,
//! and what a `read_committed` consumer sees follows.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use support::{Rig, Txn, rig, topic};

const CONCURRENT_TRANSACTIONS: i16 = 51;
const INVALID_TXN_STATE: i16 = 48;
const INVALID_PRODUCER_EPOCH: i16 = 47;
const PRODUCER_FENCED: i16 = 90;
const INVALID_PRODUCER_ID_MAPPING: i16 = 49;
const OPERATION_NOT_ATTEMPTED: i16 = 55;
const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;

/// What group `g` has committed for `t-0`: -1 if nothing.
async fn committed(rig: &Rig) -> i64 {
    rig.fetch_offsets(false, "g", Some(&[("t", &[0])]))
        .await
        .first()
        .map_or(-1, |o| o.2)
}

async fn started(timeout_ms: i32) -> (Rig, Txn) {
    let rig = rig();
    rig.create(vec![topic("t", 2), topic("u", 1)], false).await;
    let t = rig.init_txn("tx", timeout_ms).await.unwrap();
    (rig, t)
}

#[tokio::test]
async fn a_committed_transaction_is_read_after_it_ends_and_not_before() {
    let (rig, t) = started(60_000).await;
    assert_eq!(rig.add_partitions("tx", t, "t", &[0]).await, [0]);
    assert_eq!(rig.produce_txn("tx", t, ("t", 0), 0, 3).await.error_code, 0);
    // Open: a read-committed consumer is given nothing and told the last stable offset is where the transaction began.
    let open = rig.fetch_committed("t", 0, 0).await;
    assert_eq!(
        (
            open.last_stable_offset,
            open.records.as_ref().map(|r| r.len())
        ),
        (0, Some(0))
    );
    assert_eq!(
        rig.list_offset(7, "t", 0, -1).await.offset,
        3,
        "read-uncommitted sees the end"
    );
    assert_eq!(rig.end_txn("tx", t, true).await, 0);
    let done = rig.fetch_committed("t", 0, 0).await;
    assert_eq!(
        done.last_stable_offset, 4,
        "the three records and the marker"
    );
    assert!(done.records.as_ref().is_some_and(|r| !r.is_empty()));
    assert_eq!(done.aborted_transactions.as_ref().map(Vec::len), Some(0));
}

#[tokio::test]
async fn an_aborted_transaction_is_listed_so_that_a_consumer_skips_its_records() {
    let (rig, t) = started(60_000).await;
    rig.add_partitions("tx", t, "t", &[0]).await;
    rig.produce_txn("tx", t, ("t", 0), 0, 2).await;
    assert_eq!(rig.end_txn("tx", t, false).await, 0);
    let got = rig.fetch_committed("t", 0, 0).await;
    let aborted: Vec<_> = got
        .aborted_transactions
        .unwrap()
        .iter()
        .map(|a| (a.producer_id.0, a.first_offset))
        .collect();
    assert_eq!(aborted, [(t.producer, 0)]);
    assert_eq!(got.last_stable_offset, 3);
    // Past it, nothing is listed.
    assert!(
        rig.fetch_committed("t", 0, 3)
            .await
            .aborted_transactions
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_batch_needs_an_open_transaction_that_the_partition_was_added_to() {
    let (rig, t) = started(60_000).await;
    // No transaction yet; then one that has not been told of this partition.
    assert_eq!(
        rig.produce_txn("tx", t, ("t", 0), 0, 1).await.error_code,
        INVALID_TXN_STATE
    );
    rig.add_partitions("tx", t, "t", &[0]).await;
    assert_eq!(
        rig.produce_txn("tx", t, ("t", 1), 0, 1).await.error_code,
        INVALID_TXN_STATE
    );
    assert_eq!(rig.produce_txn("tx", t, ("t", 0), 0, 1).await.error_code, 0);
    // And not by a producer that does not own the id.
    let stranger = Txn {
        producer: t.producer + 1,
        epoch: 0,
    };
    assert_eq!(
        rig.produce_txn("tx", stranger, ("t", 0), 0, 1)
            .await
            .error_code,
        INVALID_PRODUCER_ID_MAPPING
    );
    assert_eq!(
        rig.add_partitions("tx", t, "nope", &[0]).await,
        [UNKNOWN_TOPIC_OR_PARTITION]
    );
    assert_eq!(
        rig.add_partitions("tx", t, "t", &[0, 9]).await,
        [OPERATION_NOT_ATTEMPTED, UNKNOWN_TOPIC_OR_PARTITION],
        "one that cannot be added leaves the rest not attempted"
    );
}

#[tokio::test]
async fn a_new_owner_of_the_id_fences_the_old_and_aborts_what_it_left_open() {
    let (rig, old) = started(60_000).await;
    rig.add_partitions("tx", old, "t", &[0]).await;
    rig.produce_txn("tx", old, ("t", 0), 0, 2).await;
    let new = rig.init_txn("tx", 60_000).await.unwrap();
    assert_eq!(
        (new.producer, new.epoch),
        (old.producer, old.epoch + 2),
        "the abort raised the epoch once, the new owner once"
    );
    // The old owner is fenced at every turn, and its transaction is gone.
    assert_eq!(
        rig.add_partitions("tx", old, "t", &[0]).await,
        [PRODUCER_FENCED]
    );
    assert_eq!(
        rig.produce_txn("tx", old, ("t", 0), 2, 1).await.error_code,
        INVALID_PRODUCER_EPOCH
    );
    assert_eq!(rig.end_txn("tx", old, true).await, PRODUCER_FENCED);
    let got = rig.fetch_committed("t", 0, 0).await;
    assert_eq!(
        got.last_stable_offset, 3,
        "the open transaction was aborted: records and a marker"
    );
    assert_eq!(got.aborted_transactions.unwrap().len(), 1);
}

#[tokio::test]
async fn each_end_is_answered_as_kafka_answers_it() {
    let (rig, t) = started(60_000).await;
    assert_eq!(
        rig.end_txn("tx", t, true).await,
        INVALID_TXN_STATE,
        "no transaction is open"
    );
    rig.add_partitions("tx", t, "t", &[0]).await;
    rig.produce_txn("tx", t, ("t", 0), 0, 1).await;
    assert_eq!(rig.end_txn("tx", t, true).await, 0);
    assert_eq!(
        rig.end_txn("tx", t, true).await,
        0,
        "a retry of an end that succeeded"
    );
    assert_eq!(
        rig.end_txn("tx", t, false).await,
        INVALID_TXN_STATE,
        "the other decision"
    );
    assert_eq!(
        rig.end_txn("nobody", t, true).await,
        INVALID_PRODUCER_ID_MAPPING
    );
    // The next transaction of the same producer starts afresh.
    rig.add_partitions("tx", t, "t", &[1]).await;
    assert_eq!(rig.produce_txn("tx", t, ("t", 1), 0, 1).await.error_code, 0);
    assert_eq!(rig.end_txn("tx", t, false).await, 0);
    assert_eq!(rig.end_txn("tx", t, false).await, 0);
    let _ = CONCURRENT_TRANSACTIONS;
}

#[tokio::test]
async fn the_offsets_a_transaction_carries_are_stored_if_it_commits_and_forgotten_if_it_aborts() {
    let (rig, t) = started(60_000).await;
    // Not added to the transaction yet.
    assert_eq!(
        rig.txn_offset("tx", t, "g", ("t", 0), 5).await,
        INVALID_TXN_STATE
    );
    assert_eq!(rig.add_offsets("tx", t, "g").await, 0);
    assert_eq!(rig.txn_offset("tx", t, "g", ("t", 0), 5).await, 0);
    assert_eq!(committed(&rig).await, -1, "staged, not stored");
    assert_eq!(rig.end_txn("tx", t, true).await, 0);
    assert_eq!(committed(&rig).await, 5);
    // An abort keeps the old.
    assert_eq!(rig.add_offsets("tx", t, "g").await, 0);
    assert_eq!(rig.txn_offset("tx", t, "g", ("t", 0), 9).await, 0);
    assert_eq!(rig.end_txn("tx", t, false).await, 0);
    assert_eq!(committed(&rig).await, 5);
}

#[tokio::test]
async fn a_transaction_that_outlives_its_timeout_is_aborted_and_its_producer_fenced() {
    let (rig, t) = started(1_000).await;
    rig.add_partitions("tx", t, "t", &[0]).await;
    rig.produce_txn("tx", t, ("t", 0), 0, 2).await;
    rig.broker.check_transactions().await;
    assert_eq!(
        rig.fetch_committed("t", 0, 0).await.last_stable_offset,
        0,
        "not yet"
    );
    tokio::time::sleep(std::time::Duration::from_millis(1_300)).await;
    rig.broker.check_transactions().await;
    let got = rig.fetch_committed("t", 0, 0).await;
    assert_eq!(
        (
            got.last_stable_offset,
            got.aborted_transactions.unwrap().len()
        ),
        (3, 1)
    );
    assert_eq!(
        rig.produce_txn("tx", t, ("t", 0), 2, 1).await.error_code,
        INVALID_PRODUCER_EPOCH
    );
}

#[tokio::test]
async fn an_open_transaction_is_left_open_by_a_restart_and_a_committed_one_stays_committed() {
    let (rig, t) = started(60_000).await;
    rig.add_partitions("tx", t, "t", &[0]).await;
    rig.produce_txn("tx", t, ("t", 0), 0, 2).await;
    rig.add_partitions("tx", t, "u", &[0]).await;
    rig.produce_txn("tx", t, ("u", 0), 0, 1).await;
    assert_eq!(rig.end_txn("tx", t, true).await, 0);
    let other = rig.init_txn("other", 60_000).await.unwrap();
    rig.add_partitions("other", other, "t", &[1]).await;
    rig.produce_txn("other", other, ("t", 1), 0, 1).await;
    // The process dies. The committed transaction is read as committed; the open one is still open.
    let restarted = support::open_on(rig.fs.process_restart());
    restarted.broker.recover_transactions().await;
    assert_eq!(
        restarted
            .fetch_committed("t", 0, 0)
            .await
            .last_stable_offset,
        3
    );
    assert_eq!(
        restarted
            .fetch_committed("t", 1, 0)
            .await
            .last_stable_offset,
        0,
        "still open"
    );
    // Its producer carries on and ends it.
    assert_eq!(
        restarted
            .produce_txn("other", other, ("t", 1), 1, 1)
            .await
            .error_code,
        0
    );
    assert_eq!(restarted.end_txn("other", other, true).await, 0);
    assert_eq!(
        restarted
            .fetch_committed("t", 1, 0)
            .await
            .last_stable_offset,
        3
    );
}
