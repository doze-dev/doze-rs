//! Fencing as Kafka's coordinator does it: a producer the broker has aborted and fenced cannot take its id back by saying
//! the epoch it had, and asking again after a lost answer is answered alike.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::Duration;

use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::{
    DescribeTransactionsRequest, InitProducerIdRequest, ProducerId, TransactionalId,
};
use support::{Rig, Txn, rig, topic};

const PRODUCER_FENCED: i16 = 90;

fn id(s: &str) -> TransactionalId {
    TransactionalId(StrBytes::from_string(s.to_owned()))
}

async fn init_as(rig: &Rig, held: Txn) -> (i16, i64, i16) {
    let mut req = InitProducerIdRequest::default();
    req.transactional_id = Some(id("tx"));
    req.transaction_timeout_ms = 60_000;
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

#[tokio::test]
async fn a_producer_the_broker_timed_out_cannot_take_its_id_back_by_saying_the_epoch_it_had() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let held = rig.init_txn("tx", 100).await.unwrap();
    assert_eq!(rig.add_partitions("tx", held, "t", &[0]).await, [0]);
    tokio::time::sleep(Duration::from_millis(400)).await;
    rig.broker.check_transactions().await;
    // The abort raised the epoch once, to fence it; the epoch before that is neither the current nor the last.
    assert_eq!(init_as(&rig, held).await, (PRODUCER_FENCED, -1, -1));
}

#[tokio::test]
async fn an_owner_asking_while_its_transaction_is_open_is_fenced_after_the_broker_aborts_it() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let held = rig.init_txn("tx", 60_000).await.unwrap();
    assert_eq!(rig.add_partitions("tx", held, "t", &[0]).await, [0]);
    assert_eq!(init_as(&rig, held).await.0, PRODUCER_FENCED);
    // Kafka answers CONCURRENT_TRANSACTIONS while it aborts and then fences the retry; here the abort is done by the time
    // the answer is given, and what is left is the same: the transaction is over, and the old epoch is not the owner's.
    assert_eq!(state(&rig).await, "CompleteAbort");
    assert_eq!(init_as(&rig, held).await.0, PRODUCER_FENCED);
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
    assert_eq!(init_as(&rig, held).await, bumped);
    assert_eq!(
        state(&rig).await,
        "Empty",
        "ready for the producer to begin again"
    );
}
