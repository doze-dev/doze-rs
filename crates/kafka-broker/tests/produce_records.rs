//! What a Produce is held to inside the batch: its records, their keys and times, and their codec.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_wire::batch::build::BatchBuilder;
use support::{records, rig, stored, topic, with_config};

fn rec(delta: i64, key: bool) -> kafka_wire::batch::build::Rec {
    kafka_wire::batch::build::Rec {
        timestamp_delta: delta,
        offset_delta: None,
        key: key.then(|| b"k".to_vec()),
        value: Some(b"v".to_vec()),
        headers: vec![],
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[tokio::test]
async fn a_topic_that_compacts_refuses_a_record_with_no_key_in_kafkas_words() {
    let rig = rig();
    rig.create(
        vec![
            with_config(topic("log", 1), "cleanup.policy", "compact"),
            topic("plain", 1),
        ],
        false,
    )
    .await;
    let batch = BatchBuilder::from_records(now_ms(), &[rec(0, true), rec(0, false)]).build();
    let r = rig.produce(1, "log", 0, batch.clone()).await;
    assert_eq!(
        (r.error_code, r.base_offset, r.log_start_offset),
        (87, -1, 0)
    );
    assert_eq!(r.record_errors.len(), 1);
    assert_eq!(r.record_errors[0].batch_index, 1);
    assert_eq!(
        r.error_message.as_deref(),
        Some(
            "One or more records have been rejected due to 1 record errors in total, and only showing the first three errors at most: [RecordError(batchIndex=1, message='Compacted topic cannot accept message without key in topic partition log-0')]"
        )
    );
    assert_eq!(
        rig.produce(1, "plain", 0, batch).await.error_code,
        0,
        "a topic that does not compact takes it"
    );
}

#[tokio::test]
async fn a_create_time_record_outside_the_topics_bounds_of_the_clock_is_invalid_timestamp() {
    let rig = rig();
    let bounded = with_config(
        with_config(topic("t", 1), "message.timestamp.after.max.ms", "1000"),
        "message.timestamp.before.max.ms",
        "60000",
    );
    rig.create(vec![bounded], false).await;
    let at = |delta: i64| BatchBuilder::from_records(now_ms() + delta, &[rec(0, true)]).build();
    assert_eq!(rig.produce(1, "t", 0, at(0)).await.error_code, 0);
    for (label, delta) in [("too far ahead", 600_000), ("too far behind", -600_000)] {
        let r = rig.produce(1, "t", 0, at(delta)).await;
        assert_eq!((r.error_code, r.base_offset), (32, -1), "{label}");
        assert_eq!(
            r.error_message.as_deref(),
            Some("One or more records have been rejected due to invalid timestamp")
        );
        assert_eq!(r.record_errors.len(), 1, "{label}");
    }
}

#[tokio::test]
async fn a_malformed_record_is_invalid_record_and_nothing_is_stored() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    // The header counts three records; the body holds one.
    let one = kafka_wire::batch::build::encode_records(&[rec(0, true)]);
    let batch = BatchBuilder::new()
        .records(3)
        .timestamps(now_ms(), now_ms())
        .body(one)
        .build();
    let r = rig.produce(1, "t", 0, batch).await;
    assert_eq!(
        (r.error_code, r.base_offset, r.log_start_offset),
        (87, -1, 0)
    );
    assert_eq!(
        rig.produce(1, "t", 0, records(1)).await.base_offset,
        0,
        "the partition is as it was"
    );
}

#[tokio::test]
async fn the_newest_timestamp_a_log_keeps_is_the_records_and_not_the_producers_claim() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let first = now_ms() - 10_000;
    // The header claims a time a day ahead; the records say 7 seconds after the first.
    let claim = BatchBuilder::from_records(first, &[rec(0, true), rec(7_000, true)])
        .timestamps(first, first + 86_400_000)
        .build();
    assert_eq!(rig.produce(-1, "t", 0, claim).await.error_code, 0);
    let held = stored(&rig.fs, "t", 0);
    let kept = kafka_wire::batch::Batch::parse(held[0].clone())
        .expect("a stored batch is a valid one: its checksum was kept right");
    assert_eq!(kept.max_timestamp().get(), first + 7_000);
    // and a lookup by time agrees.
    assert_eq!(
        rig.list_offset(7, "t", 0, -3).await.timestamp,
        first + 7_000
    );
}

#[tokio::test]
async fn a_compressed_batch_is_read_and_stored_as_it_came() {
    use std::io::Write;
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let plain = kafka_wire::batch::build::encode_records(&[rec(0, true), rec(3, true)]);
    let mut gz = flate2_encoder();
    gz.write_all(&plain).unwrap();
    let body = gz.finish().unwrap();
    let batch = BatchBuilder::new()
        .records(2)
        .attributes(1)
        .timestamps(now_ms(), now_ms() + 3)
        .body(body.clone())
        .build();
    let r = rig.produce(-1, "t", 0, batch).await;
    assert_eq!((r.error_code, r.base_offset), (0, 0));
    let held = stored(&rig.fs.power_cut(drop_cut()), "t", 0);
    assert_eq!(
        &held[0][61..],
        &body[..],
        "the compressed records are stored untouched"
    );
    // The records inside are not readable by a lookup, which answers at the grain of the batch's header.
    assert_eq!(rig.list_offset(7, "t", 0, 0).await.offset, 0);
}

fn flate2_encoder() -> flate2::write::GzEncoder<Vec<u8>> {
    flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default())
}

fn drop_cut() -> kafka_storage::sim::Cut {
    use kafka_storage::sim::{Cut, DataCut, NameCut};
    Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 1,
    }
}

#[tokio::test]
async fn a_topic_with_no_bounds_of_its_own_refuses_a_record_stamped_more_than_an_hour_ahead() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let at = |delta: i64| BatchBuilder::from_records(now_ms() + delta, &[rec(0, true)]).build();
    assert_eq!(
        rig.produce(1, "t", 0, at(3_000_000)).await.error_code,
        0,
        "50 minutes ahead is within the default"
    );
    assert_eq!(
        rig.produce(1, "t", 0, at(4_000_000)).await.error_code,
        32,
        "over an hour ahead is not"
    );
    // Behind the clock there is no default bound: a replay of old records is not refused.
    assert_eq!(
        rig.produce(1, "t", 0, at(-86_400_000 * 365))
            .await
            .error_code,
        0
    );
}
