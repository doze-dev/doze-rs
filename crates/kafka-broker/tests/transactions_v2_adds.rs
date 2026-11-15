//! KIP-890 on the wire: what a transaction of the second version adds without being asked (TxnOffsetCommit v5 its group),
//! AddPartitionsToTxn v4 and v5 (a broker's, verifying or adding for several transactions), and a classic producer and one of
//! the second version side by side. Held to Kafka 4.4.0's answers in the oracle probe (TX-16 to TX-24).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use support::{Rig, Txn, later, parts, rig, topic};

const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;
const INVALID_REQUEST: i16 = 42;
const INVALID_PRODUCER_EPOCH: i16 = 47;
const INVALID_TXN_STATE: i16 = 48;
const INVALID_PRODUCER_ID_MAPPING: i16 = 49;
const OPERATION_NOT_ATTEMPTED: i16 = 55;
const PRODUCER_FENCED: i16 = 90;
const TRANSACTION_ABORTABLE: i16 = 120;

/// Oracle `offsets`: TxnOffsetCommit to a group not added is INVALID_TXN_STATE at v3 and TRANSACTION_ABORTABLE at v4; v5
/// adds the group (the transaction then names the offsets partition), and the offsets are committed with the transaction. A
/// stale epoch is INVALID_PRODUCER_EPOCH; AddOffsetsToTxn still works for a classic client of the same id.
#[tokio::test]
async fn a_txn_offset_commit_of_v5_adds_its_group_and_older_versions_need_it_added() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let p = rig.init_txn("tx", 60_000).await.unwrap();
    assert_eq!(
        rig.txn_offset_at(3, "tx", p, "g", ("t", 0, 5)).await,
        INVALID_TXN_STATE
    );
    assert_eq!(
        rig.txn_offset_at(4, "tx", p, "g", ("t", 0, 5)).await,
        TRANSACTION_ABORTABLE
    );
    assert_eq!(rig.describe_txn("tx").await.0, "Empty");
    assert_eq!(rig.txn_offset_at(5, "tx", p, "g", ("t", 0, 5)).await, 0);
    let (s, _, topics) = rig.describe_txn("tx").await;
    assert_eq!(s, "Ongoing");
    assert!(
        topics.iter().all(|(t, _)| t == "__consumer_offsets") && topics.len() == 1,
        "{topics:?}"
    );
    assert_eq!(
        rig.txn_offset_at(3, "tx", p, "g", ("t", 0, 6)).await,
        0,
        "the group is in it now"
    );
    let q = rig.end_v5("tx", p, true).await;
    assert_eq!(offset(&rig, "g").await, 6);
    assert_eq!(
        rig.txn_offset_at(5, "tx", p, "g", ("t", 0, 9)).await,
        INVALID_PRODUCER_EPOCH
    );
    assert_eq!(rig.add_offsets("tx", q, "g").await, 0);
    assert_eq!(rig.txn_offset_at(4, "tx", q, "g", ("t", 0, 8)).await, 0);
    let r = rig.end_v5("tx", q, false).await;
    assert_eq!(offset(&rig, "g").await, 6, "aborted");
    assert_eq!(rig.txn_offset_at(5, "tx", r, "g2", ("t", 0, 3)).await, 0);
    rig.end_v5("tx", r, true).await;
    assert_eq!(offset(&rig, "g2").await, 3);
    assert_eq!(rig.add_offsets("tx", r, "g").await, PRODUCER_FENCED);
}

/// The offset group `g` has committed for t-0 (-1: none).
async fn offset(rig: &Rig, g: &str) -> i64 {
    rig.fetch_offsets(false, g, Some(&[("t", &[0])]))
        .await
        .first()
        .map_or(-1, |o| o.2)
}

/// Oracle `batched`: AddPartitionsToTxn v4 and v5 (a broker's request, several transactions). Verify-only answers NONE for a
/// partition the transaction has and TRANSACTION_ABORTABLE for one it has not, changing nothing; adding stores them. An id
/// nobody owns or another producer id is INVALID_PRODUCER_ID_MAPPING, another epoch PRODUCER_FENCED (not turned into
/// INVALID_PRODUCER_EPOCH at these versions), an empty id INVALID_REQUEST; an unknown partition is 3 and leaves the others
/// OPERATION_NOT_ATTEMPTED.
#[tokio::test]
async fn add_partitions_of_v4_and_v5_verify_or_add_for_several_transactions() {
    let rig = rig();
    rig.create(vec![topic("t", 3)], false).await;
    let a = rig.init_txn("a", 60_000).await.unwrap();
    let b = rig.init_txn("b", 60_000).await.unwrap();
    assert_eq!(rig.add_partitions("a", a, "t", &[0]).await, [0]);
    for version in [4, 5] {
        let asked = [
            ("a", a, true, "t", &[0, 1][..]),
            ("b", b, true, "t", &[0][..]),
        ];
        assert_eq!(
            rig.add_partitions_batched(version, &asked).await,
            (
                0,
                vec![vec![0, TRANSACTION_ABORTABLE], vec![TRANSACTION_ABORTABLE]]
            ),
            "v{version}"
        );
    }
    let one = |id, t, verify, ps: &'static [i32]| [(id, t, verify, "t", ps)];
    let stranger = Txn {
        producer: a.producer + 1000,
        epoch: a.epoch,
    };
    for (asked, want) in [
        (
            one("nobody", a, true, &[0]),
            vec![INVALID_PRODUCER_ID_MAPPING],
        ),
        (one("a", later(a, 1), true, &[0]), vec![PRODUCER_FENCED]),
        (
            one("a", stranger, true, &[0]),
            vec![INVALID_PRODUCER_ID_MAPPING],
        ),
        (
            one("a", a, true, &[0, 9]),
            vec![OPERATION_NOT_ATTEMPTED, UNKNOWN_TOPIC_OR_PARTITION],
        ),
        (one("b", later(b, 1), false, &[0]), vec![PRODUCER_FENCED]),
        (one("", b, false, &[0]), vec![INVALID_REQUEST]),
        (one("", b, true, &[0]), vec![INVALID_REQUEST]),
    ] {
        assert_eq!(
            rig.add_partitions_batched(5, &asked).await,
            (0, vec![want]),
            "{asked:?}"
        );
    }
    let unknown = [("a", a, true, "none", &[0][..])];
    assert_eq!(
        rig.add_partitions_batched(5, &unknown).await,
        (0, vec![vec![UNKNOWN_TOPIC_OR_PARTITION]])
    );
    assert_eq!(
        rig.describe_txn("b").await.0,
        "Empty",
        "nothing verified was added"
    );
    assert_eq!(rig.end_txn("a", a, false).await, 0);
}

/// Oracle `batched`, adding: AddPartitionsToTxn v5 not verify-only adds the partitions (at the second transaction version)
/// and begins the transaction; a write of the classic shape to them is taken; after the end, verifying them is
/// TRANSACTION_ABORTABLE again.
#[tokio::test]
async fn add_partitions_of_v5_that_is_not_verify_only_adds_and_begins() {
    let rig = rig();
    rig.create(vec![topic("t", 3)], false).await;
    let b = rig.init_txn("b", 60_000).await.unwrap();
    let one = |id, t, verify, ps: &'static [i32]| [(id, t, verify, "t", ps)];
    assert_eq!(
        rig.add_partitions_batched(5, &one("b", b, false, &[1, 2]))
            .await,
        (0, vec![vec![0, 0]])
    );
    assert_eq!(
        rig.describe_txn("b").await,
        ("Ongoing".into(), b, parts("t", &[1, 2]))
    );
    assert_eq!(
        rig.add_partitions_batched(5, &one("b", b, true, &[1]))
            .await,
        (0, vec![vec![0]])
    );
    assert_eq!(
        rig.produce_txn_at((11, "b"), b, ("t", 1), 0, 1)
            .await
            .error_code,
        0
    );
    let b = rig.end_v5("b", b, true).await;
    assert_eq!(
        rig.add_partitions_batched(5, &one("b", b, true, &[1]))
            .await,
        (0, vec![vec![TRANSACTION_ABORTABLE]]),
        "the end took the partitions out"
    );
}

/// Mixed clients: a classic one (AddPartitionsToTxn v3, Produce v9, EndTxn v3) and one of the second version (Produce v12,
/// EndTxn v5) write to the same partition at once, each on its own id; each is judged by its own protocol and both commit.
#[tokio::test]
async fn a_classic_producer_and_one_of_the_second_version_share_a_partition() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let classic = rig.init_txn("classic", 60_000).await.unwrap();
    let bumping = rig.init_txn("bumping", 60_000).await.unwrap();
    assert_eq!(rig.add_partitions("classic", classic, "t", &[0]).await, [0]);
    assert_eq!(
        rig.produce_txn("classic", classic, ("t", 0), 0, 2)
            .await
            .error_code,
        0
    );
    assert_eq!(
        rig.produce_txn_at((12, "bumping"), bumping, ("t", 0), 0, 2)
            .await
            .error_code,
        0
    );
    assert_eq!(rig.committed_view("t", 0).await, (0, 4, 0));
    assert_eq!(rig.end_txn("classic", classic, true).await, 0);
    assert_eq!(
        rig.committed_view("t", 0).await.0,
        2,
        "the other is still open"
    );
    assert_eq!(
        rig.end_v5("bumping", bumping, true).await,
        later(bumping, 1)
    );
    assert_eq!(rig.committed_view("t", 0).await, (6, 6, 0));
    assert_eq!(rig.producers_on("t", 0).await, {
        let mut want = vec![
            (classic.producer, classic.epoch, -1),
            (bumping.producer, bumping.epoch + 1, -1),
        ];
        want.sort_unstable();
        want
    });
}
