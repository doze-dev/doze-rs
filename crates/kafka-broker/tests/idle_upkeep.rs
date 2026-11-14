//! An idle broker's chores leave alone what cannot need them: the cleaner does not reopen a compacted partition it found
//! clean until it is written to or the time its look named, and the retention sweep closes what it opened and does not open
//! a partition again before anything in it could expire. "Leave alone" is measured as no file operation at all.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::Duration;

use kafka_wire::batch::build::{BatchBuilder, Rec};
use support::{open_on, rig, topic, with_config};

/// One keyed record stamped now (the cleaner's lags and retention's age are judged from record times).
fn now_keyed(key: &str, value: &str) -> bytes::Bytes {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let rec = Rec {
        key: Some(key.as_bytes().to_vec()),
        value: Some(value.as_bytes().to_vec()),
        ..Rec::default()
    };
    BatchBuilder::from_records(now, &[rec]).build()
}

fn with(name: &str, configs: &[(&str, &str)]) -> kafka_wire::messages::create_topics_request::CreatableTopic {
    let mut t = with_config(topic(name, 1), "segment.bytes", "1024");
    for (k, v) in configs {
        t = with_config(t, k, v);
    }
    t
}

async fn fill(rig: &support::Rig, topic: &str, n: usize) {
    for i in 0..n {
        let one = now_keyed(&format!("k{}", i % 5), &format!("v{i}"));
        assert_eq!(rig.produce(-1, topic, 0, one).await.error_code, 0);
    }
}

#[tokio::test]
async fn a_compacted_partition_found_clean_is_not_opened_again_until_it_is_written_to() {
    let rig = rig();
    rig.create(
        vec![
            with("c", &[("cleanup.policy", "compact")]),
            with("empty", &[("cleanup.policy", "compact")]),
        ],
        false,
    )
    .await;
    fill(&rig, "c", 60).await;
    let again = open_on(rig.fs.process_restart());
    // The first look opens it, plans (the dirty part is under the default ratio of a half once cleaned), and closes it.
    again.broker.check_cleaner().await;
    again.broker.check_cleaner().await;
    assert_eq!(again.broker.open_partitions(), 0);
    let before = again.fs.ops();
    for _ in 0..5 {
        again.broker.check_cleaner().await;
    }
    assert_eq!(
        again.fs.ops(),
        before,
        "nothing written, no lag set: the passes touch no file, the empty partition's directory included"
    );
    assert_eq!(again.broker.open_partitions(), 0);
    // Writes are changes: the next pass looks again, finds the log due, and cleans it (the partition, open now, stays open).
    let before = served(&again, "c").await;
    fill(&again, "c", 60).await;
    again.broker.check_cleaner().await;
    let after = served(&again, "c").await;
    assert!(
        after < before + 60,
        "cleaned after the writes: {after} records served, {before} before 60 more"
    );
    assert_eq!(again.broker.open_partitions(), 1);
}

/// How many records the partition serves from its start.
async fn served(rig: &support::Rig, topic: &str) -> usize {
    let start = rig.list_offset(7, topic, 0, -2).await.offset;
    let data = rig.fetch_now(&[(topic, 0, start, 1 << 20)]).await.remove(0);
    let mut kept = 0;
    for b in kafka_wire::batch::Batches::new(data.records.unwrap()) {
        b.unwrap().each_record(1 << 20, |_| kept += 1).unwrap();
    }
    kept
}

#[tokio::test]
async fn a_partition_held_back_by_the_minimum_lag_is_left_alone_until_the_lag_has_passed_and_cleaned_then() {
    let rig = rig();
    rig.create(
        vec![with(
            "c",
            &[
                ("cleanup.policy", "compact"),
                ("min.cleanable.dirty.ratio", "0.01"),
                ("min.compaction.lag.ms", "600"),
            ],
        )],
        false,
    )
    .await;
    fill(&rig, "c", 60).await;
    let again = open_on(rig.fs.process_restart());
    again.broker.check_cleaner().await;
    assert_eq!(again.broker.open_partitions(), 0, "looked at, too young, closed");
    let before = again.fs.ops();
    again.broker.check_cleaner().await;
    assert_eq!(again.fs.ops(), before, "not before the lag has passed");
    tokio::time::sleep(Duration::from_millis(700)).await;
    again.broker.check_cleaner().await;
    assert!(again.fs.ops() > before, "looked at once the lag has passed");
    let kept = served(&again, "c").await;
    assert!(kept < 60, "cleaned once due: {kept} of 60 records left");
}

#[tokio::test]
async fn the_retention_sweep_closes_what_it_opened_and_leaves_a_partition_alone_until_something_in_it_can_expire() {
    let rig = rig();
    rig.create(
        vec![
            with("aged", &[("retention.ms", "3600000")]),
            with("sized", &[("retention.bytes", "3500")]),
            with("empty", &[("retention.ms", "1000")]),
        ],
        false,
    )
    .await;
    fill(&rig, "aged", 40).await;
    fill(&rig, "sized", 40).await;
    let again = open_on(rig.fs.process_restart());
    assert_eq!(again.broker.open_partitions(), 0);
    again.broker.check_logs().await;
    assert_eq!(
        again.broker.open_partitions(),
        0,
        "what the sweep opened is closed again (it used to stay open until the limit)"
    );
    let before = again.fs.ops();
    for _ in 0..5 {
        again.broker.check_logs().await;
    }
    assert_eq!(
        again.fs.ops(),
        before,
        "an hour's age bound, a size bound not reached and an empty partition: nothing to open"
    );
    // Written past its bound: looked at again and trimmed, and left open (the write opened it, not the sweep).
    assert_eq!(again.list_offset(7, "sized", 0, -2).await.offset, 0);
    fill(&again, "sized", 40).await;
    again.broker.check_logs().await;
    assert!(
        again.list_offset(7, "sized", 0, -2).await.offset > 0,
        "the oldest segments over the bound went"
    );
    assert_eq!(again.broker.open_partitions(), 1);
}

#[tokio::test]
async fn the_retention_sweep_comes_back_when_the_oldest_segment_has_aged_out() {
    let rig = rig();
    rig.create(vec![with("aged", &[("retention.ms", "600")])], false)
        .await;
    fill(&rig, "aged", 40).await;
    let again = open_on(rig.fs.process_restart());
    again.broker.check_logs().await;
    assert_eq!(again.list_offset(7, "aged", 0, -2).await.offset, 0, "too young");
    tokio::time::sleep(Duration::from_millis(700)).await;
    again.broker.check_logs().await;
    assert!(
        again.list_offset(7, "aged", 0, -2).await.offset > 0,
        "aged out, and deleted"
    );
}

#[tokio::test(start_paused = true)]
async fn a_partition_nothing_asks_for_is_closed_after_five_quiet_minutes_and_opens_again_when_asked() {
    let rig = support::rig();
    let closer = rig.broker.start_partition_closer();
    rig.create(vec![topic("t", 2)], false).await;
    assert_eq!(rig.produce(-1, "t", 0, support::records(3)).await.error_code, 0);
    tokio::time::sleep(Duration::from_secs(60)).await;
    assert_eq!(rig.produce(-1, "t", 1, support::records(2)).await.error_code, 0);
    assert_eq!(rig.broker.open_partitions(), 2);
    // Partition 0 was last asked for a minute before partition 1: it goes first, five minutes after its last use.
    tokio::time::sleep(Duration::from_secs(239)).await;
    assert_eq!(rig.broker.open_partitions(), 2, "not before five minutes");
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(rig.broker.open_partitions(), 1, "partition 0 is closed");
    tokio::time::sleep(Duration::from_secs(60)).await;
    assert_eq!(rig.broker.open_partitions(), 0, "and partition 1 a minute later");
    // With none open the closer waits for an open, not a timer: a day passes without it waking.
    let wakes = rig.broker.background_wakes();
    tokio::time::sleep(Duration::from_secs(24 * 3600)).await;
    assert_eq!(rig.broker.background_wakes(), wakes, "no wake while idle");
    // Asked for again: opened with what it held, and closed again after five more quiet minutes.
    assert_eq!(rig.produce(-1, "t", 0, support::records(1)).await.base_offset, 3);
    assert_eq!(rig.broker.open_partitions(), 1);
    tokio::time::sleep(Duration::from_secs(301)).await;
    assert_eq!(rig.broker.open_partitions(), 0);
    closer.abort();
}

#[tokio::test(start_paused = true)]
async fn a_partition_held_by_a_request_is_not_closed_however_quiet() {
    let rig = support::rig_with(|c| c.partition_idle_close = Duration::from_secs(10));
    let closer = rig.broker.start_partition_closer();
    rig.create(vec![topic("t", 1)], false).await;
    assert_eq!(rig.produce(-1, "t", 0, support::records(3)).await.error_code, 0);
    // A fetch that waits for data it does not have holds the partition while it waits.
    let waiting = rig.fetch(support::fetch_request(&[("t", 0, 3, 1 << 20)], 60_000, 1, 1 << 20));
    let watch = async {
        tokio::time::sleep(Duration::from_secs(30)).await;
        rig.broker.open_partitions()
    };
    let (_, open_while_held) = tokio::join!(waiting, watch);
    assert_eq!(open_while_held, 1, "held by the waiting fetch");
    tokio::time::sleep(Duration::from_secs(11)).await;
    assert_eq!(rig.broker.open_partitions(), 0, "let go, then quiet: closed");
    closer.abort();
}

#[tokio::test]
async fn the_transaction_coordinator_does_not_wake_with_nothing_to_do_and_aborts_a_timed_out_transaction_on_time() {
    let rig = support::rig();
    let coordinator = rig.broker.start_transactions();
    rig.create(vec![topic("t", 1)], false).await;
    // The first look, at the start, finds nothing and sets no time.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let first = rig.broker.background_wakes();
    assert_eq!(first, 1);
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    assert_eq!(
        rig.broker.background_wakes(),
        first,
        "no transactional id: no wake (it used to look twice a second)"
    );
    // A transaction of 400 ms: a producer id given (a change, which sets the id's expiry a week away), a transaction
    // begun (which sets its timeout), and nothing else. It is aborted by the coordinator's own task, on time, after one wake.
    let t = rig.init_txn("tx", 400).await.unwrap();
    rig.add_partitions("tx", t, "t", &[0]).await;
    rig.produce_txn("tx", t, ("t", 0), 0, 2).await;
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        rig.fetch_committed("t", 0, 0).await.last_stable_offset,
        0,
        "still open"
    );
    tokio::time::sleep(Duration::from_millis(450)).await;
    let got = rig.fetch_committed("t", 0, 0).await;
    assert_eq!(
        (
            got.last_stable_offset,
            got.aborted_transactions.unwrap().len()
        ),
        (3, 1),
        "aborted by the coordinator, its marker written"
    );
    assert_eq!(
        rig.broker.background_wakes(),
        first + 1,
        "one wake: at the timeout, not every half second"
    );
    assert_eq!(rig.end_txn("tx", t, true).await, 90, "the producer is fenced");
    coordinator.abort();
}
