//! ListOffsets.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use support::{rig, topic};

#[tokio::test]
async fn list_offsets_answers_earliest_latest_the_first_record_since_a_time_and_the_newest_record()
{
    let rig = rig();
    rig.create(vec![topic("t", 1), topic("empty", 1)], false)
        .await;
    let batch_at = |records: u32, ts: i64| {
        kafka_wire::batch::build::BatchBuilder::new()
            .records(records)
            .timestamps(ts, ts)
            .build()
    };
    rig.produce(1, "t", 0, batch_at(3, 1_000)).await;
    rig.produce(1, "t", 0, batch_at(2, 2_000)).await;
    let ask = |ts: i64| rig.list_offset(7, "t", 0, ts);
    let (earliest, latest, newest) = (ask(-2).await, ask(-1).await, ask(-3).await);
    assert_eq!(
        (earliest.offset, earliest.timestamp, earliest.leader_epoch),
        (0, -1, 0)
    );
    assert_eq!((latest.offset, latest.timestamp), (5, -1));
    assert_eq!((newest.offset, newest.timestamp), (3, 2_000));
    let since = ask(1_500).await;
    assert_eq!(
        (since.offset, since.timestamp, since.error_code),
        (3, 2_000, 0)
    );
    let first = ask(0).await;
    assert_eq!((first.offset, first.timestamp), (0, 1_000));
    // After every record: offset -1 and timestamp -1, with no leader epoch.
    let none = ask(9_999).await;
    assert_eq!(
        (
            none.offset,
            none.timestamp,
            none.leader_epoch,
            none.error_code
        ),
        (-1, -1, -1, 0)
    );
    let empty = rig.list_offset(7, "empty", 0, -1).await;
    assert_eq!((empty.offset, empty.error_code), (0, 0));
    assert_eq!(rig.list_offset(7, "empty", 0, -3).await.offset, -1);
}

#[tokio::test]
async fn list_offsets_refuses_what_it_cannot_answer_in_kafkas_codes() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    assert_eq!(rig.list_offset(7, "nope", 0, -1).await.error_code, 3);
    assert_eq!(rig.list_offset(7, "t", 4, -1).await.error_code, 3);
    assert_eq!(
        rig.list_offset(6, "t", 0, -3).await.error_code,
        35,
        "max timestamp came with v7"
    );
    assert_eq!(
        rig.list_offset(7, "t", 0, -4).await.error_code,
        35,
        "-4 is beyond what v7 knows"
    );
}

#[tokio::test]
async fn list_offsets_holds_the_isolation_level_and_the_leader_epoch_to_what_a_single_node_has() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    assert_eq!(rig.list_offset_with("t", -1, 0, -1).await.error_code, 0);
    assert_eq!(
        rig.list_offset_with("t", -1, 1, 0).await.error_code,
        0,
        "read-committed, epoch 0"
    );
    assert_eq!(
        rig.list_offset_with("t", -1, 2, -1).await.error_code,
        -1,
        "an isolation level that is neither"
    );
    assert_eq!(
        rig.list_offset_with("t", -1, 0, 1).await.error_code,
        75,
        "an epoch from the future"
    );
}

#[tokio::test]
async fn asking_the_offsets_of_a_partition_never_written_to_does_not_open_it() {
    let rig = rig();
    rig.create(vec![topic("t", 2)], false).await;
    let dir = std::path::Path::new(support::DATA).join("t-0");
    for (timestamp, offset) in [(-1, 0), (-2, 0), (-3, -1), (1_000, -1)] {
        let r = rig.list_offset(7, "t", 0, timestamp).await;
        assert_eq!(
            (r.error_code, r.offset),
            (0, offset),
            "timestamp {timestamp}"
        );
    }
    let made = kafka_storage::fs::Fs::list_dir(&rig.fs, &dir).unwrap();
    assert!(made.is_empty(), "no segment was made by asking: {made:?}");
    // Once it has data it is opened and answered from the log.
    rig.produce(1, "t", 0, support::records(3)).await;
    assert_eq!(rig.list_offset(7, "t", 0, -1).await.offset, 3);
}
