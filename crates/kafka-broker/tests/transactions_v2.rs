//! KIP-890 on the wire (Produce v12 and EndTxn v5, and the refusals of an unverified write), held to what Apache Kafka 4.4.0
//! answered the oracle probe (each test names its scenario; the rules are TX-16 to TX-24 of
//! `docs/port/behaviours/transactions.md`). AddPartitionsToTxn v4 and v5 and TxnOffsetCommit v5 are in `transactions_v2_adds`;
//! the epochs running out and the new owner in `transactions_v2_epochs`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use support::{Txn, later, parts, rig, topic};

const OUT_OF_ORDER_SEQUENCE_NUMBER: i16 = 45;
const INVALID_PRODUCER_EPOCH: i16 = 47;
const INVALID_TXN_STATE: i16 = 48;
const INVALID_PRODUCER_ID_MAPPING: i16 = 49;
const PRODUCER_FENCED: i16 = 90;
const TRANSACTION_ABORTABLE: i16 = 120;

/// Oracle `basic`: Produce v12 adds its partition and begins the transaction; EndTxn v5 raises the epoch and says so; the
/// markers carry the raised epoch, so the partition's producer is at it and the old epoch is refused there; the next
/// transaction runs at the new epoch from sequence 0.
#[tokio::test]
async fn a_produce_of_v12_adds_its_partition_and_an_end_of_v5_raises_the_epoch() {
    let rig = rig();
    rig.create(vec![topic("t", 2)], false).await;
    let p = rig.init_txn("tx", 60_000).await.unwrap();
    assert_eq!(
        rig.produce_txn_at((12, "tx"), p, ("t", 0), 0, 1)
            .await
            .error_code,
        0
    );
    assert_eq!(
        rig.describe_txn("tx").await,
        ("Ongoing".into(), p, parts("t", &[0]))
    );
    assert_eq!(rig.committed_view("t", 0).await, (0, 1, 0));
    assert_eq!(
        rig.produce_txn_at((12, "tx"), p, ("t", 0), 1, 1)
            .await
            .error_code,
        0
    );
    assert_eq!(
        rig.produce_txn_at((12, "tx"), p, ("t", 1), 0, 1)
            .await
            .error_code,
        0
    );
    assert_eq!(rig.end_v5("tx", p, true).await, later(p, 1));
    assert_eq!(
        rig.describe_txn("tx").await,
        ("CompleteCommit".into(), later(p, 1), vec![])
    );
    assert_eq!(
        rig.producers_on("t", 0).await,
        [(p.producer, p.epoch + 1, -1)]
    );
    assert_eq!(rig.committed_view("t", 0).await, (3, 3, 0));
    let old = rig.produce_txn_at((12, "tx"), p, ("t", 0), 2, 1).await;
    assert_eq!(old.error_code, INVALID_PRODUCER_EPOCH);
    let q = later(p, 1);
    assert_eq!(
        rig.produce_txn_at((12, "tx"), q, ("t", 0), 0, 1)
            .await
            .error_code,
        0
    );
    assert_eq!(rig.end_v5("tx", q, false).await, later(p, 2));
    assert_eq!(rig.committed_view("t", 0).await, (5, 5, 1));
    assert_eq!(
        rig.producers_on("t", 0).await,
        [(p.producer, p.epoch + 2, -1)]
    );
    assert_eq!(
        rig.producers_on("t", 1).await,
        [(p.producer, p.epoch + 1, -1)]
    );
}

/// Oracle `table`: every cell of `TransactionCoordinator.endTransaction` for `TV_2` in one run (the classic
/// EndTxn after it is `a_classic_end_after_one_of_v5_keeps_the_epoch`).
#[tokio::test]
async fn each_end_of_v5_is_answered_as_kafka_4_answers_it() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let p = rig.init_txn("tx", 60_000).await.unwrap();
    let none = Txn {
        producer: -1,
        epoch: -1,
    };
    let end = |t: Txn, commit: bool| rig.end_txn_at(5, "tx", t, commit);
    let p0 = later(p, 0);
    let refused = |code| (code, none);
    // Empty: the epoch before is no retry there.
    assert_eq!(end(p0, true).await, refused(INVALID_TXN_STATE));
    assert_eq!(end(later(p, -1), true).await, refused(PRODUCER_FENCED));
    assert_eq!(end(later(p, -1), false).await, refused(PRODUCER_FENCED));
    assert_eq!(
        end(p0, false).await,
        (0, later(p, 1)),
        "an abort of nothing raises the epoch"
    );
    assert_eq!(
        rig.describe_txn("tx").await,
        ("CompleteAbort".into(), later(p, 1), vec![])
    );
    // CompleteAbort at p+1: p is the retry.
    assert_eq!(end(p0, true).await, refused(INVALID_TXN_STATE));
    assert_eq!(end(p0, false).await, (0, later(p, 1)));
    assert_eq!(end(later(p, 1), true).await, refused(INVALID_TXN_STATE));
    assert_eq!(end(later(p, 6), false).await, refused(PRODUCER_FENCED));
    let stranger = Txn {
        producer: p.producer + 1000,
        epoch: p.epoch + 1,
    };
    assert_eq!(
        end(stranger, false).await,
        refused(INVALID_PRODUCER_ID_MAPPING)
    );
    // Ongoing at p+1.
    assert_eq!(
        rig.produce_txn_at((12, "tx"), later(p, 1), ("t", 0), 0, 1)
            .await
            .error_code,
        0
    );
    assert_eq!(end(p0, false).await, refused(PRODUCER_FENCED));
    assert_eq!(end(later(p, 1), true).await, (0, later(p, 2)));
    // CompleteCommit at p+2: p+1 is the retry.
    assert_eq!(end(later(p, 1), true).await, (0, later(p, 2)));
    assert_eq!(end(later(p, 1), false).await, refused(INVALID_TXN_STATE));
    assert_eq!(end(later(p, 2), true).await, refused(INVALID_TXN_STATE));
    assert_eq!(end(later(p, 2), false).await, (0, later(p, 3)));
    assert_eq!(
        rig.describe_txn("tx").await,
        ("CompleteAbort".into(), later(p, 3), vec![])
    );
}

/// Oracle `table`, its end: the classic EndTxn (v4) on what the second version left keeps the epoch (its markers carry the
/// epoch the batches had) and its answer names no producer; an EndTxn v5 after it at the same epoch is no retry.
#[tokio::test]
async fn a_classic_end_after_one_of_v5_keeps_the_epoch() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let p = rig.init_txn("tx", 60_000).await.unwrap();
    let none = Txn {
        producer: -1,
        epoch: -1,
    };
    let q = rig.end_v5("tx", p, false).await;
    assert_eq!(q, later(p, 1));
    assert_eq!(rig.end_txn_at(4, "tx", q, false).await, (0, none));
    assert_eq!(rig.end_txn_at(4, "tx", p, false).await.0, PRODUCER_FENCED);
    let r = rig.produce_txn_at((12, "tx"), q, ("t", 0), 0, 1).await;
    assert_eq!(r.error_code, 0);
    assert_eq!(rig.end_txn_at(4, "tx", q, true).await, (0, none));
    assert_eq!(
        rig.describe_txn("tx").await,
        ("CompleteCommit".into(), q, vec![])
    );
    assert_eq!(rig.producers_on("t", 0).await, [(q.producer, q.epoch, -1)]);
    // A v5 end after the classic one: the current epoch is no retry.
    assert_eq!(
        rig.end_txn_at(5, "tx", q, true).await,
        (INVALID_TXN_STATE, none)
    );
    assert_eq!(rig.end_txn_at(5, "tx", q, false).await, (0, later(q, 1)));
}

/// Oracle `unadded`: a write to a partition the transaction does not have is INVALID_TXN_STATE to v10 and
/// TRANSACTION_ABORTABLE at v11, whether a transaction is open or not; v12 adds it. A classic end keeps the epoch, so a v12
/// write after it begins the next transaction at that epoch.
#[tokio::test]
async fn a_write_to_a_partition_not_added_is_refused_by_version_and_added_from_v12() {
    let rig = rig();
    rig.create(vec![topic("t", 3)], false).await;
    let p = rig.init_txn("tx", 60_000).await.unwrap();
    for (version, code) in [
        (3, INVALID_TXN_STATE),
        (9, INVALID_TXN_STATE),
        (10, INVALID_TXN_STATE),
        (11, TRANSACTION_ABORTABLE),
    ] {
        let r = rig.produce_txn_at((version, "tx"), p, ("t", 0), 0, 1).await;
        assert_eq!(r.error_code, code, "v{version}, nothing open");
    }
    assert_eq!(rig.describe_txn("tx").await.0, "Empty");
    assert_eq!(rig.add_partitions("tx", p, "t", &[1]).await, [0]);
    assert_eq!(
        rig.produce_txn_at((9, "tx"), p, ("t", 0), 0, 1)
            .await
            .error_code,
        INVALID_TXN_STATE
    );
    assert_eq!(
        rig.produce_txn_at((11, "tx"), p, ("t", 0), 0, 1)
            .await
            .error_code,
        TRANSACTION_ABORTABLE
    );
    assert_eq!(
        rig.produce_txn_at((9, "tx"), p, ("t", 1), 0, 1)
            .await
            .error_code,
        0
    );
    assert_eq!(
        rig.produce_txn_at((12, "tx"), p, ("t", 2), 0, 1)
            .await
            .error_code,
        0
    );
    assert_eq!(
        rig.describe_txn("tx").await,
        ("Ongoing".into(), p, parts("t", &[1, 2]))
    );
    assert_eq!(rig.end_txn("tx", p, true).await, 0);
    assert_eq!(
        rig.produce_txn_at((11, "tx"), p, ("t", 1), 1, 1)
            .await
            .error_code,
        TRANSACTION_ABORTABLE
    );
    assert_eq!(
        rig.produce_txn_at((12, "tx"), p, ("t", 1), 1, 1)
            .await
            .error_code,
        0
    );
    assert_eq!(rig.end_v5("tx", p, true).await, later(p, 1));
    assert_eq!(rig.committed_view("t", 1).await, (4, 4, 0));
}

/// Oracle `refusals`: Produce v12 refusals. A first batch not at sequence 0 is OUT_OF_ORDER_SEQUENCE_NUMBER, its partition
/// added all the same (Kafka adds before it appends); another epoch, lower or higher, is INVALID_PRODUCER_EPOCH; another
/// producer id or an id nobody owns INVALID_PRODUCER_ID_MAPPING; none of those adds anything. An idempotent write in the same
/// request shape is not judged by any of it.
#[tokio::test]
async fn the_refusals_of_a_produce_of_v12() {
    let rig = rig();
    rig.create(vec![topic("t", 3)], false).await;
    let p = rig.init_txn("tx", 60_000).await.unwrap();
    let p = rig.end_v5("tx", p, false).await;
    let r = rig.produce_txn_at((12, "tx"), p, ("t", 0), 5, 1).await;
    assert_eq!(r.error_code, OUT_OF_ORDER_SEQUENCE_NUMBER);
    assert_eq!(
        rig.describe_txn("tx").await,
        ("Ongoing".into(), p, parts("t", &[0]))
    );
    assert_eq!(
        rig.produce_txn_at((12, "tx"), p, ("t", 0), 0, 1)
            .await
            .error_code,
        0
    );
    for (who, code) in [
        (later(p, -1), INVALID_PRODUCER_EPOCH),
        (later(p, 1), INVALID_PRODUCER_EPOCH),
        (
            Txn {
                producer: p.producer + 1000,
                epoch: p.epoch,
            },
            INVALID_PRODUCER_ID_MAPPING,
        ),
    ] {
        let r = rig.produce_txn_at((12, "tx"), who, ("t", 1), 0, 1).await;
        assert_eq!(r.error_code, code, "{who:?}");
    }
    for version in [11, 12] {
        let r = rig
            .produce_txn_at((version, "nobody"), p, ("t", 1), 0, 1)
            .await;
        assert_eq!(r.error_code, INVALID_PRODUCER_ID_MAPPING, "v{version}");
    }
    assert_eq!(
        rig.describe_txn("tx").await,
        ("Ongoing".into(), p, parts("t", &[0]))
    );
    assert_eq!(rig.end_v5("tx", p, false).await, later(p, 1));
    assert_eq!(rig.committed_view("t", 0).await, (2, 2, 1));
    assert!(
        rig.producers_on("t", 1).await.is_empty(),
        "nothing was written to p1"
    );
}

/// The add of a Produce v12 whose record cannot be stored (the disk refuses the write) is NOT_ENOUGH_REPLICAS, retriable
/// (`ReplicaManager.handleProduceAppend` turns the coordinator's NOT_COORDINATOR into it), and nothing is added or written;
/// the store stays refused until the broker is started again (the metadata database after a failed write), and then the same
/// request goes through.
#[tokio::test]
async fn an_add_of_a_produce_of_v12_that_cannot_be_stored_is_retriable() {
    const NOT_ENOUGH_REPLICAS: i16 = 19;
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let p = rig.init_txn("tx", 60_000).await.unwrap();
    // The first operation of the request is the store of the record that adds the partition.
    rig.fs.fail_any(1, kafka_storage::sim::Failure::Io);
    let r = rig.produce_txn_at((12, "tx"), p, ("t", 0), 0, 1).await;
    assert_eq!(r.error_code, NOT_ENOUGH_REPLICAS);
    assert_eq!(rig.describe_txn("tx").await, ("Empty".into(), p, vec![]));
    assert_eq!(rig.committed_view("t", 0).await, (0, 0, 0));
    let rig = support::open_on(rig.fs.process_restart());
    let r = rig.produce_txn_at((12, "tx"), p, ("t", 0), 0, 1).await;
    assert_eq!(r.error_code, 0);
}
