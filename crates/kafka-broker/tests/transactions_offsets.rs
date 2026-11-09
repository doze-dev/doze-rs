//! TxnOffsetCommit's checks of the producer and of the committer's place in a classic group.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::Duration;

use support::{Txn, join_request, rig_with, topic};

const INVALID_PRODUCER_EPOCH: i16 = 47;
const INVALID_PRODUCER_ID_MAPPING: i16 = 49;
const ILLEGAL_GENERATION: i16 = 22;
const UNKNOWN_MEMBER_ID: i16 = 25;
const REBALANCE_IN_PROGRESS: i16 = 27;

async fn started() -> (support::Rig, Txn) {
    let rig = rig_with(|c| c.group_initial_rebalance_delay = Duration::ZERO);
    rig.create(vec![topic("t", 1)], false).await;
    let t = rig.init_txn("tx", 60_000).await.unwrap();
    assert_eq!(rig.add_offsets("tx", t, "g").await, 0);
    (rig, t)
}

#[tokio::test]
async fn offsets_from_a_producer_that_is_not_the_owner_are_refused_and_staged_for_nobody() {
    let (rig, t) = started().await;
    let at = ("t", 0, 5);
    let none = ("", -1);
    for (who, want) in [
        (
            Txn {
                epoch: t.epoch + 1,
                ..t
            },
            INVALID_PRODUCER_EPOCH,
        ),
        (
            Txn {
                epoch: t.epoch - 1,
                ..t
            },
            INVALID_PRODUCER_EPOCH,
        ),
        (
            Txn {
                producer: t.producer + 1,
                ..t
            },
            INVALID_PRODUCER_ID_MAPPING,
        ),
    ] {
        assert_eq!(
            rig.txn_offset_as("tx", who, "g", none, at).await,
            want,
            "{who:?}"
        );
    }
    // The owner's are staged, and the refused ones were not: committing leaves exactly the owner's.
    assert_eq!(rig.txn_offset_as("tx", t, "g", none, ("t", 0, 7)).await, 0);
    assert_eq!(rig.end_txn("tx", t, true).await, 0);
    let stored = rig.fetch_offsets(false, "g", Some(&[("t", &[0])])).await;
    assert_eq!(stored[0].2, 7);
}

#[tokio::test]
async fn a_member_of_a_classic_group_commits_a_transactions_offsets_at_its_generation_and_the_group_waiting_for_its_leader_does_not_stop_it()
 {
    let (rig, t) = started().await;
    let joined = rig
        .join(3, join_request("g", "", None, &[("range", "m")]))
        .await;
    let (member, generation) = (joined.member_id.as_str().to_owned(), joined.generation_id);
    // Joined and not yet synced: the group is waiting for its leader's assignment. A plain commit is told so; a
    // transaction's offsets are not refused for it.
    let plain = rig
        .commit_offsets(8, "g", &member, generation, &[("t", 0, 1, None)])
        .await;
    assert_eq!(
        plain.topics[0].partitions[0].error_code,
        REBALANCE_IN_PROGRESS
    );
    let at = |offset| ("t", 0, offset);
    assert_eq!(
        rig.txn_offset_as("tx", t, "g", (&member, generation), at(3))
            .await,
        0
    );
    // At another generation, or as a member that is not there: refused as a classic group refuses.
    assert_eq!(
        rig.txn_offset_as("tx", t, "g", (&member, generation + 1), at(4))
            .await,
        ILLEGAL_GENERATION
    );
    assert_eq!(
        rig.txn_offset_as("tx", t, "g", ("ghost", generation), at(4))
            .await,
        UNKNOWN_MEMBER_ID
    );
    // Naming nobody is accepted though the group has a member.
    assert_eq!(rig.txn_offset_as("tx", t, "g", ("", -1), at(5)).await, 0);
}
