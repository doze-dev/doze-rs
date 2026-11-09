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
