//! Fencing as Kafka's coordinator does it, at the `transaction.version` the broker finalizes: a producer the broker aborted
//! on its own may take its id back by saying the epoch it had at Kafka 4 and is fenced for good at Kafka 1 to 3; a new
//! owner fences the old one at both; asking again after a lost answer is answered alike.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::Duration;

use kafka_broker::TxnVersion;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::{
    DescribeTransactionsRequest, InitProducerIdRequest, ProducerId, TransactionalId,
};
use support::{Rig, Txn, rig, rig_with, topic};

const PRODUCER_FENCED: i16 = 90;

fn id(s: &str) -> TransactionalId {
    TransactionalId(StrBytes::from_string(s.to_owned()))
}

async fn init_as(rig: &Rig, held: Txn) -> (i16, i64, i16) {
    init_with(rig, held, 60_000).await
}

async fn init_with(rig: &Rig, held: Txn, timeout_ms: i32) -> (i16, i64, i16) {
    let mut req = InitProducerIdRequest::default();
    req.transactional_id = Some(id("tx"));
    req.transaction_timeout_ms = timeout_ms;
    req.producer_id = ProducerId(held.producer);
    req.producer_epoch = held.epoch;
    let Response::InitProducerId(r) = rig.ask(4, Request::InitProducerId(req)).await else {
        panic!("not InitProducerId")
    };
    (r.error_code, r.producer_id.0, r.producer_epoch)
}

async fn state(rig: &Rig) -> String {
    let mut req = DescribeTransactionsRequest::default();
    req.transactional_ids = vec![id("tx")];
    let Response::DescribeTransactions(r) = rig.ask(0, Request::DescribeTransactions(req)).await
    else {
        panic!("not DescribeTransactions")
    };
    r.transaction_states[0].transaction_state.to_string()
}

/// A broker standing for Kafka 1 to 3: it ends a transaction on its own the classic way.
fn classic() -> Rig {
    rig_with(|c| c.transaction_version = TxnVersion::Classic)
}

/// Kafka 4 (`transaction.version` 2) aborts a timed-out transaction as EndTxn v5 does and keeps the epoch the producer had as
/// the last, so that producer may ask for its id again with it (KIP-360) and is given the raised epoch, unbumped: the
/// oracle's `timeout` (Kafka 4.4.0: "tv1 init v4 with old epoch P1/1", and the same for a producer of the second
/// version). At the classic level the abort forgets the epoch before (`endTransactionWithTV1` sets it to -1), so the
/// producer is fenced for good.
#[tokio::test]
async fn a_producer_the_broker_timed_out_takes_its_id_back_at_kafka_4_and_is_fenced_at_the_classic_level()
 {
    for (rig, kafka_4) in [(rig(), true), (classic(), false)] {
        rig.create(vec![topic("t", 1)], false).await;
        let held = rig.init_txn("tx", 100).await.unwrap();
        assert_eq!(rig.add_partitions("tx", held, "t", &[0]).await, [0]);
        tokio::time::sleep(Duration::from_millis(400)).await;
        rig.broker.check_transactions().await;
        assert_eq!(state(&rig).await, "CompleteAbort");
        let answer = init_as(&rig, held).await;
        if kafka_4 {
            assert_eq!(answer, (0, held.producer, held.epoch + 1));
            assert_eq!(state(&rig).await, "Empty", "ready to begin again");
        } else {
            assert_eq!(answer, (PRODUCER_FENCED, -1, -1));
        }
    }
}

/// The owner asks InitProducerId with its own producer id and epoch while its transaction is open: the broker aborts the
/// transaction, raising the epoch. Kafka 4.4.0 answers CONCURRENT_TRANSACTIONS and, asked again, the raised epoch (oracle
/// `owninit`, for a producer of either version: the abort kept the epoch it had as the last); here the abort is done by
/// the time the answer is given, so the first answer is that one. At the classic level the abort forgot it: fenced.
#[tokio::test]
async fn an_owner_asking_while_its_transaction_is_open_is_given_the_raised_epoch_at_kafka_4_and_fenced_at_the_classic_level()
 {
    for (rig, kafka_4) in [(rig(), true), (classic(), false)] {
        rig.create(vec![topic("t", 1)], false).await;
        let held = rig.init_txn("tx", 60_000).await.unwrap();
        assert_eq!(rig.add_partitions("tx", held, "t", &[0]).await, [0]);
        assert_eq!(
            rig.produce_txn("tx", held, ("t", 0), 0, 1).await.error_code,
            0
        );
        let answer = init_as(&rig, held).await;
        let aborted = rig.fetch_committed("t", 0, 0).await;
        assert_eq!(
            aborted.aborted_transactions.as_ref().map_or(0, Vec::len),
            1,
            "the open transaction was aborted"
        );
        if kafka_4 {
            assert_eq!(answer, (0, held.producer, held.epoch + 1));
            assert_eq!(state(&rig).await, "Empty");
            // Asked again (the answer was lost), alike.
            assert_eq!(init_as(&rig, held).await, answer);
        } else {
            assert_eq!(answer.0, PRODUCER_FENCED);
            assert_eq!(state(&rig).await, "CompleteAbort");
            assert_eq!(init_as(&rig, held).await.0, PRODUCER_FENCED);
        }
    }
}

#[tokio::test]
async fn asking_again_with_the_epoch_before_the_last_bump_gives_the_same_answer_and_leaves_the_id_ready()
 {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let held = rig.init_txn("tx", 60_000).await.unwrap();
    let bumped = init_as(&rig, held).await;
    assert_eq!(bumped, (0, held.producer, held.epoch + 1));
    // A transaction is begun and ended at the new epoch, and the answer to the bump was lost: asking again with the old
    // epoch.
    let now = Txn {
        epoch: held.epoch + 1,
        ..held
    };
    assert_eq!(rig.add_partitions("tx", now, "t", &[0]).await, [0]);
    assert_eq!(rig.end_txn("tx", now, true).await, 0);
    assert_eq!(state(&rig).await, "CompleteCommit");
    // The asking again gives the timeout it names to the transaction the id will begin next.
    assert_eq!(init_with(&rig, held, 30_000).await, bumped);
    assert_eq!(timeout_ms(&rig).await, 30_000);
    assert_eq!(
        state(&rig).await,
        "Empty",
        "ready for the producer to begin again"
    );
}

async fn timeout_ms(rig: &Rig) -> i32 {
    let mut req = DescribeTransactionsRequest::default();
    req.transactional_ids = vec![id("tx")];
    let Response::DescribeTransactions(r) = rig.ask(0, Request::DescribeTransactions(req)).await
    else {
        panic!("not DescribeTransactions")
    };
    r.transaction_states[0].transaction_timeout_ms
}
