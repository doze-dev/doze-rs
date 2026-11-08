//! Produce and InitProducerId.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::Duration;

use bytes::Bytes;
use kafka_broker::Outcome;
use kafka_storage::sim::{Cut, DataCut, Failure, NameCut, Op};
use support::{produce_request, records, rig, stored, topic};

fn drop_unflushed() -> Cut {
    Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 1,
    }
}

/// The batch a producer sent, as the log holds it: the base offset is the log's.
fn without_offset(batch: &Bytes) -> Bytes {
    batch.slice(8..)
}

#[tokio::test]
async fn batches_are_given_the_next_offsets_in_order_and_stored_as_they_came() {
    let rig = rig();
    rig.create(vec![topic("t", 2)], false).await;
    let (a, b) = (records(3), records(2));
    let first = rig.produce(1, "t", 0, a.clone()).await;
    let second = rig.produce(1, "t", 0, b.clone()).await;
    let other = rig.produce(1, "t", 1, a.clone()).await;
    assert_eq!(
        (first.error_code, first.base_offset, first.log_start_offset),
        (0, 0, 0)
    );
    assert_eq!((second.base_offset, other.base_offset), (3, 0));
    assert_eq!(
        (first.log_append_time_ms, first.error_message.is_none()),
        (-1, true)
    );
    let held = stored(
        &rig.fs.power_cut(Cut {
            data: DataCut::Keep,
            names: NameCut::Volatile,
            seed: 1,
        }),
        "t",
        0,
    );
    assert_eq!(held.len(), 2);
    assert_eq!(without_offset(&held[0]), without_offset(&a));
    assert_eq!(
        held[1].slice(..8).as_ref(),
        3i64.to_be_bytes(),
        "the second batch starts at offset 3"
    );
}

#[tokio::test]
async fn an_unknown_topic_or_partition_is_unknown_and_nothing_is_stored() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    for (name, partition) in [("nope", 0), ("t", 1), ("t", -1)] {
        let r = rig.produce(1, name, partition, records(1)).await;
        assert_eq!(
            (r.error_code, r.base_offset, r.log_start_offset),
            (3, -1, -1),
            "{name}-{partition}"
        );
    }
}

#[tokio::test]
async fn an_acknowledged_acks_all_produce_survives_a_power_cut_and_an_acks_one_one_waits_for_the_timer()
 {
    tokio::time::pause();
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let durable = rig.produce(-1, "t", 0, records(2)).await;
    assert_eq!(durable.error_code, 0);
    let quick = rig.produce(1, "t", 0, records(1)).await;
    assert_eq!(quick.base_offset, 2);
    // Not yet flushed: a power cut now loses it, and only it.
    assert_eq!(stored(&rig.fs.power_cut(drop_unflushed()), "t", 0).len(), 1);
    // The flush comes half a second after the write, and nothing else was needed to bring it.
    tokio::time::advance(Duration::from_millis(501)).await;
    tokio::task::yield_now().await;
    assert_eq!(stored(&rig.fs.power_cut(drop_unflushed()), "t", 0).len(), 2);
}

#[tokio::test]
async fn producers_that_arrive_together_share_a_flush() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    rig.produce(-1, "t", 0, records(1)).await; // opens the partition
    let before = rig.fs.count(Op::Fsync);
    let one = || rig.produce(-1, "t", 0, records(1));
    let answers = tokio::join!(one(), one(), one(), one(), one(), one(), one(), one());
    let offsets = [
        answers.0, answers.1, answers.2, answers.3, answers.4, answers.5, answers.6, answers.7,
    ]
    .map(|r| (r.error_code, r.base_offset));
    assert_eq!(
        offsets,
        [
            (0, 1),
            (0, 2),
            (0, 3),
            (0, 4),
            (0, 5),
            (0, 6),
            (0, 7),
            (0, 8)
        ]
    );
    let flushes = rig.fs.count(Op::Fsync) - before;
    assert!(
        flushes <= 4,
        "eight producers asking for a flush at once cost {flushes} flushes, not eight"
    );
}

#[tokio::test]
async fn acks_zero_is_not_answered_unless_it_must_be_refused() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let fine = rig
        .produce_outcome(9, produce_request(0, "t", &[(0, records(2))]))
        .await;
    assert!(matches!(fine, Outcome::NoResponse));
    assert_eq!(
        rig.produce(1, "t", 0, records(1)).await.base_offset,
        2,
        "it was stored"
    );
    let refused = rig
        .produce_outcome(9, produce_request(0, "nope", &[(0, records(1))]))
        .await;
    assert!(
        matches!(refused, Outcome::Close),
        "a producer that asked for no answer is told by the connection closing"
    );
}

#[tokio::test]
async fn a_write_that_fails_is_a_storage_error_with_no_offsets_and_a_failed_flush_takes_the_partition_offline()
 {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    rig.produce(-1, "t", 0, records(1)).await;
    rig.fs.fail(Op::Fsync, 1, Failure::Io);
    let lost = rig.produce(-1, "t", 0, records(1)).await;
    assert_eq!(
        (
            lost.error_code,
            lost.base_offset,
            lost.log_start_offset,
            lost.error_message.is_none()
        ),
        (56, -1, -1, true)
    );
    // The flush is never tried again: the partition refuses until the broker restarts.
    let after = rig.produce(1, "t", 0, records(1)).await;
    assert_eq!(after.error_code, 56);
}

#[tokio::test]
async fn a_topic_deleted_while_open_takes_its_partitions_with_it_and_a_new_one_starts_at_zero() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    rig.produce(-1, "t", 0, records(5)).await;
    rig.delete_by_name(&["t"]).await;
    assert!(rig.dirs().is_empty(), "{:?}", rig.dirs());
    assert_eq!(rig.produce(1, "t", 0, records(1)).await.error_code, 3);
    rig.create(vec![topic("t", 1)], false).await;
    assert_eq!(rig.produce(1, "t", 0, records(1)).await.base_offset, 0);
}

#[tokio::test]
async fn a_topic_made_but_never_given_its_directories_is_given_them_when_it_is_first_used() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    // A cut in the middle of the create can leave a registered topic whose directories were not flushed.
    let cut = Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 1,
    };
    let fs = rig.fs.power_cut(cut);
    let empty_dir = std::path::Path::new(support::DATA).join("t-0");
    kafka_storage::fs::Fs::remove_dir(&fs, &empty_dir).unwrap();
    let after = support::open_on(fs);
    assert_eq!(after.produce(1, "t", 0, records(1)).await.error_code, 0);
}

#[tokio::test]
async fn producer_ids_are_handed_out_once_each_and_an_id_for_a_transaction_is_refused_for_now() {
    let rig = rig();
    let ids: Vec<_> = futures_seq(&rig, 3).await;
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");
    let t = rig.init_producer_id(Some("txn")).await;
    assert_eq!((t.error_code, t.producer_id.0), (42, -1));
}

async fn futures_seq(rig: &support::Rig, n: usize) -> Vec<i64> {
    let mut out = Vec::new();
    for _ in 0..n {
        let r = rig.init_producer_id(None).await;
        assert_eq!((r.error_code, r.producer_epoch), (0, 0));
        out.push(r.producer_id.0);
    }
    out
}

#[tokio::test]
async fn an_acks_all_produce_is_flushed_when_it_arrives_and_does_not_wait_for_the_timer() {
    // The timer is set to an hour: only a flush asked for by the produce itself can answer in time.
    let rig = support::rig_with(|c| c.fsync_interval = Duration::from_secs(3600));
    rig.create(vec![topic("t", 1)], false).await;
    let answered =
        tokio::time::timeout(Duration::from_secs(10), rig.produce(-1, "t", 0, records(1))).await;
    assert_eq!(
        answered
            .expect("answered without waiting an hour")
            .error_code,
        0
    );
    assert_eq!(stored(&rig.fs.power_cut(drop_unflushed()), "t", 0).len(), 1);
}
