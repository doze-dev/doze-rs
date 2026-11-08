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
