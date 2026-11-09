//! A stop at every point of ending a transaction: whatever was done, the next start leaves the transaction open (nothing
//! was decided), or decided as asked and finished on every partition with its offsets, never half of that and never the
//! other decision. The disk is the simulated one, stopped after each operation of EndTxn in turn, and then either the
//! process is killed (the disk keeps what was written) or the power goes (it keeps only what was flushed).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_storage::sim::{Cut, DataCut, NameCut, SimFs};
use support::{Rig, Txn, open_on, rig, topic};

/// A transaction with a batch on each of two partitions and offsets for group `g` staged, not yet ended.
async fn open_transaction() -> (Rig, Txn) {
    let rig = rig();
    rig.create(vec![topic("t", 2)], false).await;
    let t = rig.init_txn("tx", 60_000).await.unwrap();
    assert_eq!(rig.add_partitions("tx", t, "t", &[0, 1]).await, [0, 0]);
    assert_eq!(rig.produce_txn("tx", t, ("t", 0), 0, 2).await.error_code, 0);
    assert_eq!(rig.produce_txn("tx", t, ("t", 1), 0, 2).await.error_code, 0);
    assert_eq!(rig.add_offsets("tx", t, "g").await, 0);
    assert_eq!(rig.txn_offset("tx", t, "g", ("t", 0), 7).await, 0);
    (rig, t)
}

/// What a broker's disk says about the transaction.
#[derive(Debug, PartialEq, Eq)]
enum Seen {
    /// Nothing visible to a committed reader, offsets not stored.
    Open,
    /// Both partitions readable to the end with no aborted transaction, and the offsets stored.
    Committed,
    /// Both partitions readable to the end with their records skipped as aborted, and no offsets stored.
    Aborted,
    /// Anything else.
    Broken(String),
}

async fn seen(rig: &Rig) -> Seen {
    let (a, b) = (
        rig.fetch_committed("t", 0, 0).await,
        rig.fetch_committed("t", 1, 0).await,
    );
    let offset = rig
        .fetch_offsets(false, "g", Some(&[("t", &[0])]))
        .await
        .first()
        .map_or(-1, |o| o.2);
    let aborted = |p: &kafka_wire::messages::fetch_response::PartitionData| {
        p.aborted_transactions.as_ref().map_or(0, Vec::len)
    };
    let described = format!(
        "lso/hw {}/{} {}/{}, aborted {}+{}, offset {offset}",
        a.last_stable_offset,
        a.high_watermark,
        b.last_stable_offset,
        b.high_watermark,
        aborted(&a),
        aborted(&b)
    );
    if (a.last_stable_offset, b.last_stable_offset, offset) == (0, 0, -1) {
        Seen::Open
    } else if a.last_stable_offset == a.high_watermark
        && b.last_stable_offset == b.high_watermark
        && aborted(&a) + aborted(&b) == 0
        && offset == 7
    {
        Seen::Committed
    } else if a.last_stable_offset == a.high_watermark
        && b.last_stable_offset == b.high_watermark
        && aborted(&a) + aborted(&b) == 2
        && offset == -1
    {
        Seen::Aborted
    } else {
        Seen::Broken(described)
    }
}

async fn restarted(disk: SimFs) -> Rig {
    let rig = open_on(disk);
    rig.broker.recover_transactions().await;
    rig
}

async fn stopped_at_every_point(commit: bool) {
    let decided = if commit {
        Seen::Committed
    } else {
        Seen::Aborted
    };
    // How many operations a commit takes when nothing goes wrong.
    let (clean, t) = open_transaction().await;
    let before = clean.fs.ops();
    assert_eq!(clean.end_txn("tx", t, commit).await, 0);
    let steps = clean.fs.ops() - before;
    assert!(steps > 4, "{steps}");
    for stop in 0..=steps {
        for power in [false, true] {
            stopped_at(stop, power, commit, &decided, steps).await;
        }
    }
}

/// A commit or abort stopped after `stop` operations, by a kill or (`power`) a power cut.
async fn stopped_at(stop: u64, power: bool, commit: bool, decided: &Seen, steps: u64) {
    let (rig, t) = open_transaction().await;
    rig.fs.halt_after(stop);
    let answer = rig.end_txn("tx", t, commit).await;
    let disk = if power {
        rig.fs.power_cut(Cut {
            data: DataCut::Drop,
            names: NameCut::Durable,
            seed: stop,
        })
    } else {
        rig.fs.process_restart()
    };
    let after = restarted(disk.process_restart()).await;
    let what = format!("stop {stop}/{steps}, power {power}, EndTxn answered {answer}");
    let state = seen(&after).await;
    assert!(
        state == Seen::Open || state == *decided,
        "{what}: {state:?}"
    );
    if answer == 0 {
        assert_eq!(state, *decided, "{what}: acknowledged, so decided");
    }
    // The producer, finding the answer lost, asks again: it is decided, once.
    assert_eq!(after.end_txn("tx", t, commit).await, 0, "{what}");
    assert_eq!(seen(&after).await, *decided, "{what}");
    // And a second restart finds it as it was.
    let again = restarted(after.fs.process_restart()).await;
    assert_eq!(seen(&again).await, *decided, "{what}: again");
}

#[tokio::test]
async fn a_stop_at_any_point_of_a_commit_leaves_the_transaction_open_or_committed_everywhere() {
    stopped_at_every_point(true).await;
}

#[tokio::test]
async fn a_stop_at_any_point_of_an_abort_leaves_the_transaction_open_or_aborted_everywhere() {
    stopped_at_every_point(false).await;
}

/// The state the coordinator holds for transactional id `tx`.
async fn state_of(rig: &Rig) -> String {
    use kafka_wire::api::{Request, Response};
    use kafka_wire::messages::{DescribeTransactionsRequest, TransactionalId};
    let mut req = DescribeTransactionsRequest::default();
    req.transactional_ids = vec![TransactionalId(kafka_wire::StrBytes::from_string(
        "tx".into(),
    ))];
    let Response::DescribeTransactions(r) = rig.ask(0, Request::DescribeTransactions(req)).await
    else {
        panic!("not DescribeTransactions")
    };
    r.transaction_states[0].transaction_state.to_string()
}

/// A decision stored and not carried out (the stop came between): whoever next asks about the transaction carries it
/// out, then is answered as the decision stands: the same decision succeeds, the other is INVALID_TXN_STATE, a new owner
/// of the id finds the transaction over. (Kafka answers CONCURRENT_TRANSACTIONS for the milliseconds the markers take
/// and the client asks again; here the asker waits those milliseconds, so the answer it gets is the later one.)
#[tokio::test]
async fn a_decision_stored_and_not_carried_out_is_carried_out_by_whoever_asks_next() {
    const INVALID_TXN_STATE: i16 = 48;
    let (clean, t) = open_transaction().await;
    let before = clean.fs.ops();
    assert_eq!(clean.end_txn("tx", t, true).await, 0);
    let steps = clean.fs.ops() - before;
    let mut reached = 0;
    for stop in 0..=steps {
        for asked in ["same", "other", "owner"] {
            let (rig, t) = open_transaction().await;
            rig.fs.halt_after(stop);
            rig.end_txn("tx", t, true).await;
            // Restarted, and nothing has yet looked at the transaction.
            let after = open_on(rig.fs.process_restart());
            if state_of(&after).await != "PrepareCommit" {
                continue;
            }
            reached += 1;
            let what = format!("stop {stop}, asked {asked}");
            match asked {
                "same" => assert_eq!(after.end_txn("tx", t, true).await, 0, "{what}"),
                "other" => assert_eq!(
                    after.end_txn("tx", t, false).await,
                    INVALID_TXN_STATE,
                    "{what}"
                ),
                _ => {
                    let owner = after.init_txn("tx", 60_000).await.unwrap();
                    assert_eq!(owner.epoch, t.epoch + 1, "{what}");
                }
            }
            assert_eq!(seen(&after).await, Seen::Committed, "{what}");
        }
    }
    assert!(
        reached > 0,
        "some stop left a decision stored and not carried out"
    );
}

/// One operation of EndTxn failing (the disk full, a device error) while the process goes on: the answer is an error or
/// success, the sweeper may be asked to look, and whatever happened, the next start leaves the transaction open or
/// committed everywhere, and the producer's asking again commits it.
#[tokio::test]
async fn one_operation_failing_anywhere_in_a_commit_is_survived() {
    use kafka_storage::sim::Failure;
    let (clean, t) = open_transaction().await;
    let before = clean.fs.ops();
    assert_eq!(clean.end_txn("tx", t, true).await, 0);
    let steps = clean.fs.ops() - before;
    for (failure, power) in [
        (Failure::Full, false),
        (Failure::Io, false),
        (Failure::Io, true),
    ] {
        for nth in 1..=steps {
            let (rig, t) = open_transaction().await;
            // (With a power cut, a flush that fails loses the pages it was to flush, as the kernel may: an answer of
            // success then holds only if the marker was waited for until it was on disk.)
            rig.fs.lose_pages_on_failed_fsync(power);
            rig.fs.fail_any(nth, failure);
            let answer = rig.end_txn("tx", t, true).await;
            rig.broker.check_transactions().await;
            let what =
                format!("{failure:?} at {nth}/{steps}, power {power}, EndTxn answered {answer}");
            let disk = if power {
                rig.fs.power_cut(Cut {
                    data: DataCut::Drop,
                    names: NameCut::Durable,
                    seed: nth,
                })
            } else {
                rig.fs.process_restart()
            };
            let after = restarted(disk.process_restart()).await;
            let state = seen(&after).await;
            assert!(
                state == Seen::Open || state == Seen::Committed,
                "{what}: {state:?}"
            );
            if answer == 0 {
                assert_eq!(state, Seen::Committed, "{what}: acknowledged");
            }
            assert_eq!(after.end_txn("tx", t, true).await, 0, "{what}");
            assert_eq!(seen(&after).await, Seen::Committed, "{what}");
        }
    }
}

/// A decision stored and not carried out is a transaction that has not ended: the offsets it carries are still unstable.
#[tokio::test]
async fn offsets_of_a_decision_stored_and_not_carried_out_are_still_unstable() {
    const UNSTABLE_OFFSET_COMMIT: i16 = 88;
    let (clean, t) = open_transaction().await;
    let before = clean.fs.ops();
    assert_eq!(clean.end_txn("tx", t, true).await, 0);
    let steps = clean.fs.ops() - before;
    let mut reached = 0;
    for stop in 0..=steps {
        let (rig, t) = open_transaction().await;
        rig.fs.halt_after(stop);
        rig.end_txn("tx", t, true).await;
        let after = open_on(rig.fs.process_restart());
        if state_of(&after).await != "PrepareCommit" {
            continue;
        }
        reached += 1;
        let seen = after.fetch_as("g", None, "t", Some(&[0]), true).await;
        assert_eq!(
            seen.partitions,
            [(0, UNSTABLE_OFFSET_COMMIT, -1)],
            "stop {stop}"
        );
    }
    assert!(reached > 0);
}
