//! KIP-890 on the wire: a new owner fences a producer of the second version for good, and an end (EndTxn v5, or the broker's
//! own abort) at the last epoch of a producer id hands over to a new one. Held to Kafka 4.4.0's answers in the oracle probe
//! (TX-16 to TX-24).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use support::{Txn, later, owned_at, rig, rig_holding, topic};

const INVALID_PRODUCER_EPOCH: i16 = 47;
const INVALID_TXN_STATE: i16 = 48;
const INVALID_PRODUCER_ID_MAPPING: i16 = 49;
const PRODUCER_FENCED: i16 = 90;

/// Oracle `zombie`: a new owner takes the id over while a transaction of the second version is open: it is aborted (its
/// markers at the raised epoch), the new owner is two epochs on, and the old producer is fenced for good everywhere.
#[tokio::test]
async fn a_new_owner_fences_a_producer_of_the_second_version_for_good() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let a = rig.init_txn("tx", 60_000).await.unwrap();
    assert_eq!(
        rig.produce_txn_at((12, "tx"), a, ("t", 0), 0, 1)
            .await
            .error_code,
        0
    );
    let b = rig.init_txn("tx", 60_000).await.unwrap();
    assert_eq!(b, later(a, 2));
    assert_eq!(rig.describe_txn("tx").await, ("Empty".into(), b, vec![]));
    assert_eq!(
        rig.producers_on("t", 0).await,
        [(a.producer, a.epoch + 1, -1)]
    );
    assert_eq!(
        rig.produce_txn_at((12, "tx"), a, ("t", 0), 1, 1)
            .await
            .error_code,
        INVALID_PRODUCER_EPOCH
    );
    for commit in [false, true] {
        assert_eq!(rig.end_txn_at(5, "tx", a, commit).await.0, PRODUCER_FENCED);
    }
    assert_eq!(
        rig.produce_txn_at((12, "tx"), b, ("t", 0), 0, 1)
            .await
            .error_code,
        0
    );
    assert_eq!(rig.end_v5("tx", b, true).await, later(b, 1));
    assert_eq!(rig.committed_view("t", 0).await, (4, 4, 1));
}

/// Oracle `exhaust`: once the epochs of a producer id run out, EndTxn v5 hands over to a new producer id at epoch 0. The
/// markers carry the last epoch of the old id; asked again with the old id and its last epoch, the end is a retry (the
/// same decision succeeds with the new id, the other is INVALID_TXN_STATE); an older epoch of the old id is
/// INVALID_PRODUCER_ID_MAPPING; InitProducerId with the old id and its last epoch is answered with the new one, unbumped.
#[tokio::test]
async fn an_end_that_uses_up_the_epochs_hands_over_to_a_new_producer_id() {
    const OLD: i64 = 900_000;
    let rig = rig_holding(&owned_at("tx", OLD, i16::MAX - 3, 60_000));
    rig.create(vec![topic("t", 1)], false).await;
    let mut p = Txn {
        producer: OLD,
        epoch: i16::MAX - 3,
    };
    for _ in 0..2 {
        assert_eq!(
            rig.produce_txn_at((12, "tx"), p, ("t", 0), 0, 1)
                .await
                .error_code,
            0
        );
        p = rig.end_v5("tx", p, true).await;
    }
    assert_eq!(
        p,
        Txn {
            producer: OLD,
            epoch: i16::MAX - 1
        }
    );
    assert_eq!(
        rig.produce_txn_at((12, "tx"), p, ("t", 0), 0, 1)
            .await
            .error_code,
        0
    );
    let fresh = rig.end_v5("tx", p, true).await;
    assert!(fresh.producer != OLD && fresh.producer >= 0, "{fresh:?}");
    assert_eq!(fresh.epoch, 0);
    assert_eq!(
        rig.describe_txn("tx").await,
        ("CompleteCommit".into(), fresh, vec![])
    );
    assert_eq!(rig.producers_on("t", 0).await, [(OLD, i16::MAX, -1)]);
    assert_eq!(rig.end_txn_at(5, "tx", p, true).await, (0, fresh));
    assert_eq!(rig.end_txn_at(5, "tx", p, false).await.0, INVALID_TXN_STATE);
    assert_eq!(
        rig.end_txn_at(5, "tx", later(p, -1), true).await.0,
        INVALID_PRODUCER_ID_MAPPING
    );
    assert_eq!(rig.init_txn_holding("tx", p).await, Ok(fresh));
    assert_eq!(
        rig.describe_txn("tx").await,
        ("Empty".into(), fresh, vec![])
    );
    assert_eq!(rig.committed_view("t", 0).await, (6, 6, 0));
    // And the new producer id runs transactions as any.
    assert_eq!(
        rig.produce_txn_at((12, "tx"), fresh, ("t", 0), 0, 1)
            .await
            .error_code,
        0
    );
    assert_eq!(rig.end_v5("tx", fresh, true).await, later(fresh, 1));
}

/// The broker's own abort at Kafka 4 (a timeout) of a transaction at the last epoch of its producer id hands over too, and
/// the producer that timed out finds its abort was the retry: answered with the new producer id.
#[tokio::test]
async fn a_timeout_at_the_last_epoch_hands_over_to_a_new_producer_id() {
    const OLD: i64 = 900_000;
    let rig = rig_holding(&owned_at("tx", OLD, i16::MAX - 1, 100));
    rig.create(vec![topic("t", 1)], false).await;
    let p = Txn {
        producer: OLD,
        epoch: i16::MAX - 1,
    };
    assert_eq!(
        rig.produce_txn_at((12, "tx"), p, ("t", 0), 0, 1)
            .await
            .error_code,
        0
    );
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    rig.broker.check_transactions().await;
    let (s, fresh, _) = rig.describe_txn("tx").await;
    assert_eq!(s, "CompleteAbort");
    assert!(fresh.producer != OLD && fresh.epoch == 0, "{fresh:?}");
    assert_eq!(rig.producers_on("t", 0).await, [(OLD, i16::MAX, -1)]);
    assert_eq!(rig.committed_view("t", 0).await, (2, 2, 1));
    assert_eq!(rig.end_txn_at(5, "tx", p, false).await, (0, fresh));
}
