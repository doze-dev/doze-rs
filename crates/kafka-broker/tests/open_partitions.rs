//! A broker with many partitions keeps only some of them open.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_storage::sim::{Cut, DataCut, NameCut};
use support::{batch_bases, records, rig_with, stored, topic};

#[tokio::test]
async fn partitions_past_the_limit_are_closed_flushed_and_come_back_with_their_data() {
    let rig = rig_with(|c| c.max_open_partitions = 2);
    rig.create(vec![topic("t", 6)], false).await;
    // acks=1: nothing asks for a flush, and the timer is half a second away.
    for p in 0..6 {
        let r = rig.produce(1, "t", p, records(2 + p as u32)).await;
        assert_eq!((r.error_code, r.base_offset), (0, 0), "partition {p}");
    }
    // A power cut now loses the two partitions still open and none of the four that were closed to make room.
    let cut = rig.fs.power_cut(Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 1,
    });
    let held: Vec<usize> = (0..6).map(|p| stored(&cut, "t", p).len()).collect();
    assert_eq!(held[..4], [1, 1, 1, 1], "closing flushes: {held:?}");
    // Every partition is there for a client, however many were closed in between, and carries on from where it was.
    for p in 0..6 {
        let got = rig.fetch_now(&[("t", p, 0, 1 << 20)]).await.remove(0);
        assert_eq!(
            batch_bases(got.records.as_ref().unwrap()),
            [0],
            "partition {p}"
        );
        let next = rig.produce(1, "t", p, records(1)).await;
        assert_eq!(next.base_offset, 2 + i64::from(p), "partition {p}");
    }
}

#[tokio::test]
async fn a_clean_stop_covers_the_partitions_that_were_closed_on_the_way() {
    let rig = rig_with(|c| c.max_open_partitions = 1);
    rig.create(vec![topic("t", 3)], false).await;
    for p in 0..3 {
        rig.produce(1, "t", p, records(1)).await;
    }
    rig.broker.shutdown().await.unwrap();
    let marker = rig
        .fs
        .contents(&std::path::Path::new(support::DATA).join("clean-shutdown"))
        .unwrap();
    assert_eq!(
        String::from_utf8(marker).unwrap().lines().count(),
        1,
        "the header and no partition left unvouched"
    );
}

#[tokio::test]
async fn no_more_than_the_limit_stay_open_and_a_limit_that_fits_them_all_closes_none() {
    let tight = rig_with(|c| c.max_open_partitions = 2);
    tight.create(vec![topic("t", 6)], false).await;
    for p in 0..6 {
        tight.produce(1, "t", p, records(1)).await;
        assert_eq!(tight.broker.open_partitions(), (p as usize + 1).min(2));
    }
    let roomy = rig_with(|c| c.max_open_partitions = 6);
    roomy.create(vec![topic("t", 6)], false).await;
    for p in 0..6 {
        roomy.produce(1, "t", p, records(1)).await;
    }
    assert_eq!(roomy.broker.open_partitions(), 6);
}

#[tokio::test]
async fn deleting_a_topic_closes_its_partitions_before_removing_their_files() {
    let rig = rig_with(|c| c.max_open_partitions = 0);
    rig.create(vec![topic("t", 3), topic("keep", 1)], false)
        .await;
    for p in 0..3 {
        rig.produce(1, "t", p, records(1)).await;
    }
    rig.produce(1, "keep", 0, records(1)).await;
    assert_eq!(rig.broker.open_partitions(), 4);
    rig.delete_by_name(&["t"]).await;
    assert_eq!(rig.broker.open_partitions(), 1, "only the other topic's");
}

#[tokio::test]
async fn a_partition_whose_flush_failed_when_it_was_closed_to_make_room_is_not_vouched_for_at_the_stop()
 {
    use kafka_storage::sim::{Failure, Op};
    // Which flush is the one closing t-0 is not fixed by anything here, so every one in turn is made to fail, and
    // at least one of them must be that one; where it was, the stop has to say t-0 is not whole.
    let mut named = Vec::new();
    for nth in 1..=8 {
        let rig = rig_with(|c| c.max_open_partitions = 1);
        rig.create(vec![topic("t", 2)], false).await;
        rig.produce(1, "t", 0, records(1)).await; // acks=1: not flushed yet
        rig.fs.fail(Op::Fsync, nth, Failure::Io);
        rig.produce(1, "t", 1, records(1)).await; // opens t-1 and closes t-0
        let _ = rig.broker.shutdown().await;
        let marker = rig
            .fs
            .contents(&std::path::Path::new("/data").join("clean-shutdown"))
            .map(|m| String::from_utf8(m).unwrap())
            .unwrap_or_default();
        if marker.lines().any(|l| l == "t-0") {
            named.push(nth);
        }
    }
    assert!(!named.is_empty(), "no failed flush was reported as t-0's");
}
