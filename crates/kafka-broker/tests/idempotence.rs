//! An idempotent producer's retries: a batch sent again is answered with where it went the first time and written
//! once; a gap, a stale epoch and a repeat from long ago are refused in Kafka's codes; and the memory of it survives a
//! restart (it is relearned from the log).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use bytes::Bytes;
use kafka_wire::batch::build::BatchBuilder;
use support::{open_on, rig, topic};

const OUT_OF_ORDER: i16 = 45;
const DUPLICATE: i16 = 46;
const INVALID_EPOCH: i16 = 47;

/// A batch of `n` records from producer `id` at `epoch`, numbered from `seq`.
fn batch(id: i64, epoch: i16, seq: i32, n: u32) -> Bytes {
    BatchBuilder::new()
        .records(n)
        .timestamps(1_000, 1_000)
        .producer(id, epoch, seq)
        .build()
}

async fn rig_with_topic() -> support::Rig {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    rig
}

#[tokio::test]
async fn a_batch_sent_again_is_answered_with_its_first_offset_and_written_once() {
    let rig = rig_with_topic().await;
    let first = rig.produce(-1, "t", 0, batch(7, 0, 0, 3)).await;
    assert_eq!((first.error_code, first.base_offset), (0, 0));
    let second = rig.produce(-1, "t", 0, batch(7, 0, 3, 2)).await;
    assert_eq!((second.error_code, second.base_offset), (0, 3));
    // The first is sent again, as a producer does when it did not hear: the same offset, and nothing new written.
    let again = rig.produce(-1, "t", 0, batch(7, 0, 0, 3)).await;
    assert_eq!((again.error_code, again.base_offset), (0, 0));
    assert_eq!(
        rig.list_offset(7, "t", 0, -1).await.offset,
        5,
        "written once"
    );
    // The next one in order goes on from the end.
    let third = rig.produce(-1, "t", 0, batch(7, 0, 5, 1)).await;
    assert_eq!((third.error_code, third.base_offset), (0, 5));
}

#[tokio::test]
async fn a_gap_a_stale_epoch_and_a_repeat_from_long_ago_are_refused() {
    let rig = rig_with_topic().await;
    for i in 0..7 {
        rig.produce(-1, "t", 0, batch(7, 2, i * 2, 2)).await;
    }
    let code = |r: kafka_wire::messages::produce_response::PartitionProduceResponse| {
        (r.error_code, r.base_offset)
    };
    assert_eq!(
        code(rig.produce(-1, "t", 0, batch(7, 2, 40, 1)).await),
        (OUT_OF_ORDER, -1),
        "a gap"
    );
    assert_eq!(
        code(rig.produce(-1, "t", 0, batch(7, 1, 14, 1)).await),
        (INVALID_EPOCH, -1),
        "an older epoch"
    );
    assert_eq!(
        code(rig.produce(-1, "t", 0, batch(7, 2, 5, 2)).await),
        (OUT_OF_ORDER, -1),
        "straddling two"
    );
    assert_eq!(
        code(rig.produce(-1, "t", 0, batch(7, 2, 0, 2)).await),
        (DUPLICATE, -1),
        "older than the window"
    );
    // A new epoch starts at 0.
    assert_eq!(
        code(rig.produce(-1, "t", 0, batch(7, 3, 4, 1)).await),
        (OUT_OF_ORDER, -1)
    );
    assert_eq!(code(rig.produce(-1, "t", 0, batch(7, 3, 0, 1)).await).0, 0);
    assert_eq!(
        rig.list_offset(7, "t", 0, -1).await.offset,
        15,
        "only what was accepted"
    );
}

#[tokio::test]
async fn producers_are_judged_apart_and_a_batch_with_no_producer_is_not_judged() {
    let rig = rig_with_topic().await;
    assert_eq!(
        rig.produce(-1, "t", 0, batch(7, 0, 0, 1)).await.error_code,
        0
    );
    assert_eq!(
        rig.produce(-1, "t", 0, batch(8, 0, 0, 1)).await.error_code,
        0,
        "another producer's sequence 0"
    );
    assert_eq!(
        rig.produce(-1, "t", 0, support::records(1))
            .await
            .error_code,
        0
    );
    assert_eq!(
        rig.produce(-1, "t", 0, support::records(1))
            .await
            .error_code,
        0,
        "no producer, no sequence"
    );
}

#[tokio::test]
async fn a_retry_after_a_restart_is_still_recognised() {
    let rig = rig_with_topic().await;
    rig.produce(-1, "t", 0, batch(7, 0, 0, 3)).await;
    rig.produce(-1, "t", 0, batch(7, 0, 3, 3)).await;
    // The process dies after the writes were flushed and before the producer heard: it sends the last again.
    let restarted = open_on(rig.fs.process_restart());
    let again = restarted.produce(-1, "t", 0, batch(7, 0, 3, 3)).await;
    assert_eq!(
        (again.error_code, again.base_offset),
        (0, 3),
        "answered with where it went"
    );
    assert_eq!(
        restarted.list_offset(7, "t", 0, -1).await.offset,
        6,
        "and not written again"
    );
    let next = restarted.produce(-1, "t", 0, batch(7, 0, 6, 1)).await;
    assert_eq!((next.error_code, next.base_offset), (0, 6));
}
