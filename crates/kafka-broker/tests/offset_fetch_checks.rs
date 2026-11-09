//! OffsetFetch beyond the offsets themselves: a member that fetches (v9) must be the one it says, and a fetch for stable
//! offsets does not see what a transaction is still deciding.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use support::{Rig, Txn, join_848, rig, topic};

const UNSTABLE_OFFSET_COMMIT: i16 = 88;
const UNKNOWN_MEMBER_ID: i16 = 25;
const STALE_MEMBER_EPOCH: i16 = 113;

async fn started() -> (Rig, Txn) {
    let rig = rig();
    rig.create(vec![topic("t", 2)], false).await;
    let t = rig.init_txn("tx", 60_000).await.unwrap();
    (rig, t)
}

#[tokio::test]
async fn offsets_a_transaction_is_still_deciding_are_unstable_for_a_fetch_that_asks_for_stable_ones()
 {
    let (rig, t) = started().await;
    rig.commit_offsets(8, "g", "", -1, &[("t", 0, 3, None)])
        .await;
    assert_eq!(rig.add_offsets("tx", t, "g").await, 0);
    assert_eq!(rig.txn_offset("tx", t, "g", ("t", 0), 5).await, 0);
    let stable = rig.fetch_as("g", None, "t", Some(&[0, 1]), true).await;
    assert_eq!(
        stable.partitions,
        [(0, UNSTABLE_OFFSET_COMMIT, -1), (1, 0, -1)]
    );
    // The same fetch not asking for stable offsets is given the last committed.
    let plain = rig.fetch_as("g", None, "t", Some(&[0, 1]), false).await;
    assert_eq!(plain.partitions, [(0, 0, 3), (1, 0, -1)]);
    // Every offset the group has: the partition that has one is flagged.
    let all = rig.fetch_as("g", None, "t", None, true).await;
    assert_eq!(all.partitions, [(0, UNSTABLE_OFFSET_COMMIT, -1)]);
    // Another group is not held up by it.
    let other = rig.fetch_as("other", None, "t", Some(&[0]), true).await;
    assert_eq!(other.partitions, [(0, 0, -1)]);
    // Ended, it is no longer pending: committed or not.
    assert_eq!(rig.end_txn("tx", t, true).await, 0);
    let done = rig.fetch_as("g", None, "t", Some(&[0]), true).await;
    assert_eq!(done.partitions, [(0, 0, 5)]);
    assert_eq!(rig.add_offsets("tx", t, "g").await, 0);
    assert_eq!(rig.txn_offset("tx", t, "g", ("t", 0), 9).await, 0);
    assert_eq!(rig.end_txn("tx", t, false).await, 0);
    let aborted = rig.fetch_as("g", None, "t", Some(&[0]), true).await;
    assert_eq!(aborted.partitions, [(0, 0, 5)]);
}

#[tokio::test]
async fn a_member_fetching_the_offsets_of_a_consumer_group_must_be_a_member_at_its_epoch() {
    let rig = rig();
    rig.create(vec![topic("t", 2)], false).await;
    let joined = rig.heartbeat_848(0, join_848("cg", "m", &["t"])).await;
    assert_eq!(joined.error_code, 0);
    let epoch = joined.member_epoch;
    let ask =
        |member: Option<(&'static str, i32)>| rig.fetch_as("cg", member, "t", Some(&[0]), false);
    assert_eq!(ask(Some(("m", epoch))).await.error, 0);
    assert_eq!(ask(Some(("m", epoch + 1))).await.error, STALE_MEMBER_EPOCH);
    assert_eq!(ask(Some(("m", -1))).await.error, STALE_MEMBER_EPOCH);
    assert_eq!(ask(Some(("nobody", epoch))).await.error, UNKNOWN_MEMBER_ID);
    // An admin tool names no member: nothing is checked. Nor is a group that is not there.
    assert_eq!(ask(None).await.error, 0);
    let absent = rig
        .fetch_as("nope", Some(("m", 3)), "t", Some(&[0]), false)
        .await;
    assert_eq!(absent.error, 0);
}
