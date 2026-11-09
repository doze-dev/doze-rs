//! Transactional ids that are not used again are forgotten, in memory and in the store; ones that are in use are not.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::Duration;

use support::{open_on_with, rig_with, topic};

const EXPIRY: Duration = Duration::from_millis(200);

#[tokio::test]
async fn an_id_with_no_open_transaction_is_forgotten_after_the_expiry_and_one_with_an_open_one_is_not()
 {
    let rig = rig_with(|c| c.transactional_id_expiration = EXPIRY);
    rig.create(vec![topic("t", 1)], false).await;
    let idle = rig.init_txn("idle", 60_000).await.unwrap();
    let done = rig.init_txn("done", 60_000).await.unwrap();
    rig.add_partitions("done", done, "t", &[0]).await;
    assert_eq!(rig.end_txn("done", done, true).await, 0);
    let busy = rig.init_txn("busy", 60_000).await.unwrap();
    rig.add_partitions("busy", busy, "t", &[0]).await;
    tokio::time::sleep(EXPIRY * 2).await;
    rig.broker.check_transactions().await;
    // Forgotten: a producer using the old identity is not known, and the id is made afresh for the next to ask.
    assert_eq!(rig.end_txn("idle", idle, true).await, 49);
    assert_eq!(rig.end_txn("done", done, true).await, 49);
    let again = rig.init_txn("idle", 60_000).await.unwrap();
    assert_ne!(again.producer, idle.producer, "a new producer id");
    // The open one is still there, and still the owner's.
    assert_eq!(rig.add_partitions("busy", busy, "t", &[0]).await, [0]);
    // And the store forgot them too: after a restart only the open one comes back.
    let restarted = open_on_with(rig.fs.process_restart(), |c| {
        c.transactional_id_expiration = EXPIRY;
    });
    restarted.broker.recover_transactions().await;
    assert_eq!(restarted.end_txn("done", done, true).await, 49);
    assert_eq!(restarted.end_txn("busy", busy, false).await, 0);
}

#[tokio::test]
async fn an_id_that_is_used_again_does_not_expire_under_its_new_owner() {
    let rig = rig_with(|c| c.transactional_id_expiration = EXPIRY);
    let first = rig.init_txn("tx", 60_000).await.unwrap();
    tokio::time::sleep(EXPIRY / 2).await;
    let second = rig.init_txn("tx", 60_000).await.unwrap();
    assert_eq!(second.producer, first.producer);
    tokio::time::sleep(EXPIRY * 3 / 4).await;
    rig.broker.check_transactions().await;
    assert_eq!(rig.end_txn("tx", second, true).await, 48, "still there");
}
