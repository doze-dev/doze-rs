//! OffsetCommit and OffsetFetch through the broker.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::Duration;

use kafka_storage::sim::{Cut, DataCut, NameCut};
use support::{join_request, open_on, rig_with, topic};

const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;
const OFFSET_METADATA_TOO_LARGE: i16 = 12;
const INVALID_GROUP_ID: i16 = 24;
const ILLEGAL_GENERATION: i16 = 22;
const REBALANCE_IN_PROGRESS: i16 = 27;
const UNKNOWN_MEMBER_ID: i16 = 25;

async fn rig_with_topic() -> support::Rig {
    let rig = rig_with(|c| c.group_initial_rebalance_delay = Duration::ZERO);
    rig.create(vec![topic("t", 3), topic("u", 1)], false).await;
    rig
}

fn codes(r: &kafka_wire::messages::OffsetCommitResponse) -> Vec<Vec<i16>> {
    r.topics
        .iter()
        .map(|t| t.partitions.iter().map(|p| p.error_code).collect())
        .collect()
}

#[tokio::test]
async fn a_simple_consumers_commit_is_read_back_whole_in_both_forms() {
    let rig = rig_with_topic().await;
    let r = rig
        .commit_offsets(
            8,
            "g",
            "",
            -1,
            &[("t", 0, 42, Some("note")), ("t", 2, 7, None)],
        )
        .await;
    assert_eq!(codes(&r), [[0, 0]]);
    for batched in [false, true] {
        let got = rig
            .fetch_offsets(batched, "g", Some(&[("t", &[0, 1, 2])]))
            .await;
        assert_eq!(
            got,
            [
                ("t".into(), 0, 42, "note".into()),
                ("t".into(), 1, -1, String::new()),
                ("t".into(), 2, 7, String::new()),
            ],
            "batched: {batched}"
        );
    }
}

#[tokio::test]
async fn nothing_known_is_not_an_error_and_a_null_topic_list_means_everything_committed() {
    let rig = rig_with_topic().await;
    let got = rig
        .fetch_offsets(false, "nobody", Some(&[("t", &[0]), ("nope", &[5])]))
        .await;
    assert_eq!(
        got,
        [
            ("t".into(), 0, -1, String::new()),
            ("nope".into(), 5, -1, String::new())
        ]
    );
    assert!(rig.fetch_offsets(false, "nobody", None).await.is_empty());
    rig.commit_offsets(
        8,
        "g",
        "",
        -1,
        &[("u", 0, 3, None), ("t", 2, 2, None), ("t", 0, 1, None)],
    )
    .await;
    let all = rig.fetch_offsets(true, "g", None).await;
    let places: Vec<_> = all
        .iter()
        .map(|(t, p, o, _)| (t.as_str(), *p, *o))
        .collect();
    assert_eq!(places, [("t", 0, 1), ("t", 2, 2), ("u", 0, 3)]);
    // A group id nobody could have committed under is simply a group with nothing.
    assert!(rig.fetch_offsets(false, "", None).await.is_empty());
}

#[tokio::test]
async fn each_partition_is_judged_and_the_good_ones_are_stored() {
    let rig = rig_with_topic().await;
    let long = "x".repeat(4097);
    let r = rig
        .commit_offsets(
            8,
            "g",
            "",
            -1,
            &[
                ("t", 0, 1, None),
                ("t", 3, 2, None),
                ("t", -1, 3, None),
                ("nope", 0, 4, None),
                ("t", 1, 5, Some(long.as_str())),
                ("t", 2, 6, Some(&"y".repeat(4096))),
            ],
        )
        .await;
    assert_eq!(
        codes(&r),
        [
            vec![
                0,
                UNKNOWN_TOPIC_OR_PARTITION,
                UNKNOWN_TOPIC_OR_PARTITION,
                OFFSET_METADATA_TOO_LARGE,
                0
            ],
            vec![UNKNOWN_TOPIC_OR_PARTITION]
        ]
    );
    let stored = rig.fetch_offsets(false, "g", None).await;
    let places: Vec<_> = stored
        .iter()
        .map(|(t, p, o, _)| (t.as_str(), *p, *o))
        .collect();
    assert_eq!(places, [("t", 0, 1), ("t", 2, 6)]);
    // Negative offsets, and offsets past the end, are stored as sent.
    rig.commit_offsets(
        8,
        "g",
        "",
        -1,
        &[("t", 1, -5, None), ("u", 0, 1 << 40, None)],
    )
    .await;
    let again = rig
        .fetch_offsets(false, "g", Some(&[("t", &[1]), ("u", &[0])]))
        .await;
    assert_eq!((again[0].2, again[1].2), (-5, 1 << 40));
}

#[tokio::test]
async fn an_unusable_group_id_refuses_every_partition() {
    let rig = rig_with_topic().await;
    let r = rig
        .commit_offsets(8, "", "", -1, &[("t", 0, 1, None), ("nope", 0, 2, None)])
        .await;
    assert_eq!(codes(&r), [[INVALID_GROUP_ID], [INVALID_GROUP_ID]]);
}

#[tokio::test]
async fn a_member_commits_at_its_generation_and_a_stale_one_is_refused_and_stores_nothing() {
    let rig = rig_with_topic().await;
    let joined = rig
        .join(3, join_request("g", "", None, &[("range", "m")]))
        .await;
    let id = joined.member_id.as_str().to_owned();
    // The leader has not synced: the group is waiting for its assignment.
    let r = rig
        .commit_offsets(8, "g", &id, 1, &[("t", 0, 1, None)])
        .await;
    assert_eq!(codes(&r), [[REBALANCE_IN_PROGRESS]]);
    rig.sync(3, "g", &id, 1, &[(&id, "")]).await;
    let r = rig
        .commit_offsets(8, "g", &id, 1, &[("t", 0, 10, None)])
        .await;
    assert_eq!(codes(&r), [[0]]);
    let r = rig
        .commit_offsets(8, "g", &id, 2, &[("t", 0, 20, None)])
        .await;
    assert_eq!(codes(&r), [[ILLEGAL_GENERATION]]);
    let r = rig
        .commit_offsets(8, "g", "ghost", 1, &[("t", 0, 20, None)])
        .await;
    assert_eq!(codes(&r), [[UNKNOWN_MEMBER_ID]]);
    // A simple consumer may not overwrite the offsets of a running group.
    let r = rig
        .commit_offsets(8, "g", "", -1, &[("t", 0, 30, None)])
        .await;
    assert_eq!(codes(&r), [[UNKNOWN_MEMBER_ID]]);
    assert_eq!(
        rig.fetch_offsets(false, "g", None).await[0].2,
        10,
        "only the good commit landed"
    );
    // A group that does not exist takes only a commit that names no generation.
    let r = rig
        .commit_offsets(8, "fresh", "m", 3, &[("t", 0, 1, None)])
        .await;
    assert_eq!(codes(&r), [[ILLEGAL_GENERATION]]);
}

#[tokio::test]
async fn a_commit_that_was_acknowledged_survives_the_power_going() {
    let rig = rig_with_topic().await;
    rig.commit_offsets(8, "g", "", -1, &[("t", 1, 99, Some("kept"))])
        .await;
    let cut = Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 1,
    };
    let after = open_on(rig.fs.power_cut(cut));
    let got = after.fetch_offsets(false, "g", None).await;
    assert_eq!(got, [("t".into(), 1, 99, "kept".into())]);
}

#[tokio::test]
async fn deleting_a_topic_takes_the_offsets_committed_to_it() {
    let rig = rig_with_topic().await;
    rig.commit_offsets(8, "g", "", -1, &[("t", 0, 5, None), ("u", 0, 6, None)])
        .await;
    rig.delete_by_name(&["t"]).await;
    let got = rig.fetch_offsets(false, "g", None).await;
    assert_eq!(got, [("u".into(), 0, 6, String::new())]);
    rig.create(vec![topic("t", 3)], false).await;
    assert_eq!(
        rig.fetch_offsets(false, "g", Some(&[("t", &[0])])).await[0].2,
        -1
    );
}

#[tokio::test]
async fn an_unknown_partition_is_reported_before_oversized_metadata() {
    let rig = rig_with_topic().await;
    let long = "x".repeat(5000);
    let r = rig
        .commit_offsets(
            8,
            "g",
            "",
            -1,
            &[
                ("t", 3, 1, Some(long.as_str())),
                ("t", -1, 1, Some(long.as_str())),
            ],
        )
        .await;
    assert_eq!(
        codes(&r),
        [[UNKNOWN_TOPIC_OR_PARTITION, UNKNOWN_TOPIC_OR_PARTITION]]
    );
}

#[tokio::test]
async fn a_commit_that_cannot_be_stored_says_so_to_every_partition_it_covered() {
    use kafka_storage::sim::{Failure, Op};
    const NOT_COORDINATOR: i16 = 16;
    let rig = rig_with_topic().await;
    rig.fs.fail(Op::Fsync, 1, Failure::Io);
    let r = rig
        .commit_offsets(
            8,
            "g",
            "",
            -1,
            &[("t", 0, 1, None), ("t", 9, 2, None), ("u", 0, 3, None)],
        )
        .await;
    assert_eq!(
        codes(&r),
        [
            vec![NOT_COORDINATOR, UNKNOWN_TOPIC_OR_PARTITION],
            vec![NOT_COORDINATOR]
        ]
    );
}
