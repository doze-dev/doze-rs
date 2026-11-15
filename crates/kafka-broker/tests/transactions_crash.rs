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
use support::{Rig, Txn, open_on, owned_at, rig, rig_holding, topic};

/// How the producer speaks: the classic protocol (AddPartitionsToTxn, Produce v9, AddOffsetsToTxn, EndTxn v3), or KIP-890's
/// second part (Produce v12 and TxnOffsetCommit v5 add what they write to; EndTxn v5 raises the epoch), or that at the last
/// epoch of its producer id, so that the end hands over to a new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Protocol {
    Classic,
    Bumping,
    Exhausting,
}

/// The producer id the exhausting case starts with (seeded, at the last epoch it may hand out).
const OLD: i64 = 900_000;

/// A transaction with a batch on each of two partitions and offsets for group `g` staged, not yet ended.
async fn open_transaction() -> (Rig, Txn) {
    open_transaction_of(Protocol::Classic).await
}

async fn open_transaction_of(protocol: Protocol) -> (Rig, Txn) {
    let rig = match protocol {
        Protocol::Exhausting => rig_holding(&owned_at("tx", OLD, i16::MAX - 1, 60_000)),
        _ => rig(),
    };
    rig.create(vec![topic("t", 2)], false).await;
    let t = match protocol {
        Protocol::Exhausting => Txn {
            producer: OLD,
            epoch: i16::MAX - 1,
        },
        _ => rig.init_txn("tx", 60_000).await.unwrap(),
    };
    if protocol == Protocol::Classic {
        write_classic(&rig, t).await;
    } else {
        for p in [0, 1] {
            let r = rig.produce_txn_at((12, "tx"), t, ("t", p), 0, 2).await;
            assert_eq!(r.error_code, 0);
        }
        assert_eq!(rig.txn_offset_at(5, "tx", t, "g", ("t", 0, 7)).await, 0);
    }
    (rig, t)
}

/// The writes of a classic producer: the partitions and the group added first.
async fn write_classic(rig: &Rig, t: Txn) {
    assert_eq!(rig.add_partitions("tx", t, "t", &[0, 1]).await, [0, 0]);
    assert_eq!(rig.produce_txn("tx", t, ("t", 0), 0, 2).await.error_code, 0);
    assert_eq!(rig.produce_txn("tx", t, ("t", 1), 0, 2).await.error_code, 0);
    assert_eq!(rig.add_offsets("tx", t, "g").await, 0);
    assert_eq!(rig.txn_offset("tx", t, "g", ("t", 0), 7).await, 0);
}

/// EndTxn as the producer of `protocol` sends it: its code.
async fn end(rig: &Rig, protocol: Protocol, t: Txn, commit: bool) -> i16 {
    match protocol {
        Protocol::Classic => rig.end_txn("tx", t, commit).await,
        _ => rig.end_txn_at(5, "tx", t, commit).await.0,
    }
}

/// The producer the record holds once the transaction is decided: the same at the classic protocol, the epoch raised by
/// one at the second, a new producer id at 0 (any but the old one) when the epochs ran out.
async fn after_the_end(rig: &Rig, protocol: Protocol, t: Txn) {
    let (_, now, _) = rig.describe_txn("tx").await;
    match protocol {
        Protocol::Classic => assert_eq!(now, t),
        Protocol::Bumping => assert_eq!(
            now,
            Txn {
                epoch: t.epoch + 1,
                ..t
            }
        ),
        Protocol::Exhausting => assert!(now.producer != OLD && now.epoch == 0, "{now:?}"),
    }
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
    stopped_at_every_point_of(Protocol::Classic, commit).await;
}

async fn stopped_at_every_point_of(protocol: Protocol, commit: bool) {
    let decided = if commit {
        Seen::Committed
    } else {
        Seen::Aborted
    };
    // How many operations a commit takes when nothing goes wrong.
    let (clean, t) = open_transaction_of(protocol).await;
    let before = clean.fs.ops();
    assert_eq!(end(&clean, protocol, t, commit).await, 0);
    let steps = clean.fs.ops() - before;
    assert!(steps > 4, "{steps}");
    for stop in 0..=steps {
        for power in [false, true] {
            stopped_at(protocol, (stop, power), commit, &decided, steps).await;
        }
    }
}

/// A commit or abort stopped after `stop` operations, by a kill or (`power`) a power cut.
async fn stopped_at(
    protocol: Protocol,
    (stop, power): (u64, bool),
    commit: bool,
    decided: &Seen,
    steps: u64,
) {
    let (rig, t) = open_transaction_of(protocol).await;
    rig.fs.halt_after(stop);
    let answer = end(&rig, protocol, t, commit).await;
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
    let what =
        format!("{protocol:?}: stop {stop}/{steps}, power {power}, EndTxn answered {answer}");
    let state = seen(&after).await;
    assert!(
        state == Seen::Open || state == *decided,
        "{what}: {state:?}"
    );
    if answer == 0 {
        assert_eq!(state, *decided, "{what}: acknowledged, so decided");
    }
    // The producer, finding the answer lost, asks again with the epoch it had (at the second version, a retry of the end
    // that raised it): it is decided, once, and the producer is handed what to go on with.
    assert_eq!(end(&after, protocol, t, commit).await, 0, "{what}");
    assert_eq!(seen(&after).await, *decided, "{what}");
    after_the_end(&after, protocol, t).await;
    let handed = after.describe_txn("tx").await.1;
    // And a second restart finds it as it was, the producer it handed over to included.
    let again = restarted(after.fs.process_restart()).await;
    assert_eq!(seen(&again).await, *decided, "{what}: again");
    assert_eq!(again.describe_txn("tx").await.1, handed, "{what}: again");
}

#[tokio::test]
async fn a_stop_at_any_point_of_a_commit_leaves_the_transaction_open_or_committed_everywhere() {
    stopped_at_every_point(true).await;
}

/// The same for EndTxn v5 (KIP-890's second part): the decision raises the epoch, the markers carry the raised one, and the
/// producer asking again with the epoch it had is answered as a retry, once decided, whatever the stop.
#[tokio::test]
async fn a_stop_at_any_point_of_a_commit_of_v5_leaves_it_open_or_committed_at_the_raised_epoch() {
    stopped_at_every_point_of(Protocol::Bumping, true).await;
}

#[tokio::test]
async fn a_stop_at_any_point_of_an_abort_of_v5_leaves_it_open_or_aborted_at_the_raised_epoch() {
    stopped_at_every_point_of(Protocol::Bumping, false).await;
}

/// An EndTxn v5 at the last epoch of a producer id: the new producer id is chosen and stored with the decision, so a stop
/// anywhere hands over to one producer id, once, and the producer asking again with the old id and its last epoch is told
/// it.
#[tokio::test]
async fn a_stop_at_any_point_of_a_commit_that_uses_up_the_epochs_hands_over_once() {
    stopped_at_every_point_of(Protocol::Exhausting, true).await;
}

#[tokio::test]
async fn a_stop_at_any_point_of_an_abort_that_uses_up_the_epochs_hands_over_once() {
    stopped_at_every_point_of(Protocol::Exhausting, false).await;
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

/// An abort of EndTxn v5 stored and not carried out has raised the epoch already, which fences whoever had it: a new owner
/// arriving finds it carried out as decided, not raised again, and is given the epoch after (Kafka answers
/// CONCURRENT_TRANSACTIONS until its markers are written and then bumps once). A classic abort stored is raised to fence
/// and then bumped: the same two epochs on.
#[tokio::test]
async fn a_new_owner_after_a_stored_abort_of_v5_is_two_epochs_on_as_after_a_classic_one() {
    for protocol in [Protocol::Classic, Protocol::Bumping] {
        let (clean, t) = open_transaction_of(protocol).await;
        let before = clean.fs.ops();
        assert_eq!(end(&clean, protocol, t, false).await, 0);
        let steps = clean.fs.ops() - before;
        let mut reached = 0;
        for stop in 0..=steps {
            let (rig, t) = open_transaction_of(protocol).await;
            rig.fs.halt_after(stop);
            end(&rig, protocol, t, false).await;
            let after = open_on(rig.fs.process_restart());
            if state_of(&after).await != "PrepareAbort" {
                continue;
            }
            reached += 1;
            let owner = after.init_txn("tx", 60_000).await.unwrap();
            assert_eq!(
                owner,
                Txn {
                    epoch: t.epoch + 2,
                    ..t
                },
                "{protocol:?}, stop {stop}"
            );
            assert_eq!(
                seen(&after).await,
                Seen::Aborted,
                "{protocol:?}, stop {stop}"
            );
        }
        assert!(reached > 0, "{protocol:?}");
    }
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
    for protocol in [Protocol::Classic, Protocol::Bumping, Protocol::Exhausting] {
        one_operation_failing_in(protocol).await;
    }
}

async fn one_operation_failing_in(protocol: Protocol) {
    use kafka_storage::sim::Failure;
    let (clean, t) = open_transaction_of(protocol).await;
    let before = clean.fs.ops();
    assert_eq!(end(&clean, protocol, t, true).await, 0);
    let steps = clean.fs.ops() - before;
    for (failure, power) in [
        (Failure::Full, false),
        (Failure::Io, false),
        (Failure::Io, true),
    ] {
        for nth in 1..=steps {
            let (rig, t) = open_transaction_of(protocol).await;
            // (With a power cut, a flush that fails loses the pages it was to flush, as the kernel may: an answer of
            // success then holds only if the marker was waited for until it was on disk.)
            rig.fs.lose_pages_on_failed_fsync(power);
            rig.fs.fail_any(nth, failure);
            let answer = end(&rig, protocol, t, true).await;
            rig.broker.check_transactions().await;
            let what = format!(
                "{protocol:?}: {failure:?} at {nth}/{steps}, power {power}, EndTxn answered {answer}"
            );
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
            assert_eq!(end(&after, protocol, t, true).await, 0, "{what}");
            assert_eq!(seen(&after).await, Seen::Committed, "{what}");
            after_the_end(&after, protocol, t).await;
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
