//! Retention acts: the oldest segments a topic's `retention.bytes` or `retention.ms` no longer keeps are deleted by the
//! broker's sweep, and a consumer below the new start is told so.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use support::{records, rig, topic, with_config};

const OFFSET_OUT_OF_RANGE: i16 = 1;

/// A partition of small segments, a dozen batches of ten records to a segment or so.
async fn filled(configs: &[(&str, &str)]) -> support::Rig {
    let rig = rig();
    let mut t = with_config(topic("t", 1), "segment.bytes", "1024");
    for (k, v) in configs {
        t = with_config(t, k, v);
    }
    rig.create(vec![t], false).await;
    for _ in 0..60 {
        assert_eq!(rig.produce(-1, "t", 0, records(10)).await.error_code, 0);
    }
    rig
}

#[tokio::test]
async fn a_topic_over_its_retention_bytes_loses_its_oldest_segments_and_a_reader_below_the_new_start_is_told()
 {
    let rig = filled(&[("retention.bytes", "5000")]).await;
    assert_eq!(rig.list_offset(7, "t", 0, -2).await.offset, 0);
    rig.broker.check_logs().await;
    let start = rig.list_offset(7, "t", 0, -2).await.offset;
    assert!(start > 0 && start < 600, "the start moved to {start}");
    assert_eq!(
        rig.list_offset(7, "t", 0, -1).await.offset,
        600,
        "the end did not"
    );
    let below = rig
        .fetch(support::fetch_request(
            &[("t", 0, 0, 1 << 20)],
            0,
            1,
            1 << 20,
        ))
        .await;
    assert_eq!(below[0].error_code, OFFSET_OUT_OF_RANGE);
    let at = rig
        .fetch(support::fetch_request(
            &[("t", 0, start, 1 << 20)],
            0,
            1,
            1 << 20,
        ))
        .await;
    assert_eq!(at[0].error_code, 0);
    assert_eq!(at[0].log_start_offset, start);
}

#[tokio::test]
async fn a_topic_with_no_bound_keeps_everything_and_one_whose_age_is_up_loses_what_is_older() {
    let rig = filled(&[]).await;
    rig.broker.check_logs().await;
    assert_eq!(rig.list_offset(7, "t", 0, -2).await.offset, 0, "no bound");
    // The records are stamped with a time long ago: retention.ms of a second finds every sealed segment too old.
    let old = filled(&[("retention.ms", "1000")]).await;
    old.broker.check_logs().await;
    assert!(old.list_offset(7, "t", 0, -2).await.offset > 0);
}

#[tokio::test]
async fn the_active_segment_is_never_deleted_whatever_the_bound() {
    let rig = filled(&[("retention.bytes", "0")]).await;
    rig.broker.check_logs().await;
    let start = rig.list_offset(7, "t", 0, -2).await.offset;
    assert!(start < 600, "the newest data stays: start {start}");
    let got = rig
        .fetch(support::fetch_request(
            &[("t", 0, start, 1 << 20)],
            0,
            1,
            1 << 20,
        ))
        .await;
    assert!(got[0].records.as_ref().is_some_and(|r| !r.is_empty()));
}
