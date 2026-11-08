//! Stopping the broker cleanly, and what the next start makes of it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::path::Path;

use kafka_storage::fs::{Fs, FsFile};
use kafka_storage::sim::{Cut, DataCut, NameCut, SimFs};
use support::{DATA, open_on, records, rig, stored, topic};

fn lose_what_was_not_flushed(fs: &SimFs) -> SimFs {
    fs.power_cut(Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 1,
    })
}

#[tokio::test]
async fn a_clean_stop_flushes_what_acks_one_left_in_the_page_cache() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    rig.produce(1, "t", 0, records(2)).await;
    assert!(
        stored(&lose_what_was_not_flushed(&rig.fs), "t", 0).is_empty(),
        "not yet flushed"
    );
    rig.broker.shutdown().await.unwrap();
    let after = lose_what_was_not_flushed(&rig.fs);
    assert_eq!(stored(&after, "t", 0).len(), 1, "flushed by the stop");
    assert!(
        after
            .contents(&Path::new(DATA).join("clean-shutdown"))
            .is_some(),
        "and the stop is recorded, durably"
    );
}

#[tokio::test]
async fn the_next_start_trusts_a_clean_stop_and_not_a_killed_process() {
    // Thirty batches of a hundred kilobytes, and a byte flipped inside the fifth after the stop: damage a scan of the
    // whole log finds and cuts at, and that a start trusting the last stop does not look for.
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let big = kafka_wire::batch::build::Rec {
        timestamp_delta: 0,
        offset_delta: None,
        key: None,
        value: Some(vec![7; 100_000]),
        headers: vec![],
    };
    let batch = kafka_wire::batch::build::BatchBuilder::from_records(0, &[big]).build();
    for _ in 0..30 {
        rig.produce(1, "t", 0, batch.clone()).await;
    }
    rig.broker.shutdown().await.unwrap();
    let damaged = |fs: SimFs| {
        let log = Path::new(DATA).join("t-0").join(format!("{:020}.log", 0));
        let file = fs.open_existing(&log).unwrap();
        file.write_all_at(&[0xff], 4 * batch.len() as u64 + 200)
            .unwrap();
        file.sync_data().unwrap();
        fs
    };
    let clean = damaged(lose_what_was_not_flushed(&rig.fs));
    // Killed instead: the same disk, without the marker a clean stop writes.
    let killed = damaged(lose_what_was_not_flushed(&rig.fs));
    killed
        .remove_file(&Path::new(DATA).join("clean-shutdown"))
        .unwrap();
    let next =
        |fs: SimFs| async move { open_on(fs).produce(1, "t", 0, records(1)).await.base_offset };
    assert_eq!(
        next(clean).await,
        30,
        "the start that trusts the stop does not look inside the log"
    );
    assert_eq!(
        next(killed).await,
        4,
        "the start that does not scans it, and cuts the log at the damage"
    );
}

#[tokio::test]
async fn a_stop_does_not_vouch_for_a_partition_it_never_opened_after_an_unclean_one() {
    let rig = rig();
    rig.create(vec![topic("a", 1), topic("b", 1)], false).await;
    rig.produce(1, "a", 0, records(1)).await;
    rig.produce(1, "b", 0, records(1)).await;
    // Killed (no stop): the next run starts distrusting both, opens only "a", and stops cleanly.
    let second = open_on(rig.fs.process_restart());
    second.produce(1, "a", 0, records(1)).await;
    second.broker.shutdown().await.unwrap();
    let marker = second
        .fs
        .contents(&Path::new(DATA).join("clean-shutdown"))
        .unwrap();
    let text = String::from_utf8(marker).unwrap();
    assert!(
        text.lines().any(|l| l == "b-0"),
        "b-0 was never opened, so it is still not vouched for: {text:?}"
    );
    assert!(
        !text.lines().any(|l| l == "a-0"),
        "a-0 was flushed and closed: {text:?}"
    );
}
