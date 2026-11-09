//! DeleteRecords: what Kafka answers, in its order (read from its source), and what it leaves.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_wire::api::{Request, Response};
use kafka_wire::messages::delete_records_request::{DeleteRecordsPartition, DeleteRecordsTopic};
use kafka_wire::messages::{DeleteRecordsRequest, DeleteRecordsResponse};
use support::{Rig, Txn, open_on, records, rig, topic, wire_name, with_config};

const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;
const OFFSET_OUT_OF_RANGE: i16 = 1;
const POLICY_VIOLATION: i16 = 44;

async fn delete(rig: &Rig, version: i16, asks: &[(&str, &[(i32, i64)])]) -> DeleteRecordsResponse {
    let mut req = DeleteRecordsRequest::default();
    req.timeout_ms = 5_000;
    for (name, partitions) in asks {
        let mut t = DeleteRecordsTopic::default();
        t.name = wire_name(name);
        for (index, offset) in *partitions {
            let mut p = DeleteRecordsPartition::default();
            p.partition_index = *index;
            p.offset = *offset;
            t.partitions.push(p);
        }
        req.topics.push(t);
    }
    let Response::DeleteRecords(r) = rig.ask(version, Request::DeleteRecords(req)).await else {
        panic!("not DeleteRecords")
    };
    r
}

/// (error, low watermark) of the one partition asked.
async fn one(rig: &Rig, topic: &str, partition: i32, offset: i64) -> (i16, i64) {
    let r = delete(rig, 2, &[(topic, &[(partition, offset)])]).await;
    let p = &r.topics[0].partitions[0];
    (p.error_code, p.low_watermark)
}

async fn filled(rig: &Rig, batches: usize) {
    rig.create(vec![topic("t", 2)], false).await;
    for _ in 0..batches {
        rig.produce(-1, "t", 0, records(10)).await;
    }
}

#[tokio::test]
async fn the_start_moves_to_the_offset_asked_and_a_reader_below_it_is_told() {
    let rig = rig();
    filled(&rig, 5).await; // offsets 0 to 49
    assert_eq!(one(&rig, "t", 0, 25).await, (0, 25));
    assert_eq!(rig.list_offset(7, "t", 0, -2).await.offset, 25);
    let below = rig
        .fetch(support::fetch_request(
            &[("t", 0, 10, 1 << 20)],
            0,
            1,
            1 << 20,
        ))
        .await;
    assert_eq!(below[0].error_code, OFFSET_OUT_OF_RANGE);
    assert_eq!(below[0].log_start_offset, -1);
    let at = rig
        .fetch(support::fetch_request(
            &[("t", 0, 25, 1 << 20)],
            0,
            1,
            1 << 20,
        ))
        .await;
    assert_eq!((at[0].error_code, at[0].log_start_offset), (0, 25));
    // An offset at or below the start changes nothing and says where the start is.
    assert_eq!(one(&rig, "t", 0, 25).await, (0, 25));
    assert_eq!(one(&rig, "t", 0, 3).await, (0, 25));
    // -1 is the high watermark: the partition is emptied.
    assert_eq!(one(&rig, "t", 0, -1).await, (0, 50));
    assert_eq!(
        rig.list_offset(7, "t", 0, -1).await.offset,
        50,
        "the end did not move"
    );
}

#[tokio::test]
async fn an_offset_past_the_high_watermark_or_below_minus_one_is_out_of_range_and_a_missing_partition_is_unknown()
 {
    let rig = rig();
    filled(&rig, 2).await;
    // Offset 0 is the start already: nothing is deleted, and the start stays 0 (not 1).
    assert_eq!(one(&rig, "t", 0, 0).await, (0, 0));
    assert_eq!(rig.list_offset(7, "t", 0, -2).await.offset, 0);
    assert_eq!(one(&rig, "t", 0, 21).await, (OFFSET_OUT_OF_RANGE, -1));
    assert_eq!(
        one(&rig, "t", 0, 20).await,
        (0, 20),
        "the high watermark itself is allowed"
    );
    assert_eq!(one(&rig, "t", 0, -2).await, (OFFSET_OUT_OF_RANGE, -1));
    assert_eq!(one(&rig, "t", 7, 0).await, (UNKNOWN_TOPIC_OR_PARTITION, -1));
    // The topic has partitions 0 and 1: the number 2 is the first that is not one.
    assert_eq!(one(&rig, "t", 2, 0).await, (UNKNOWN_TOPIC_OR_PARTITION, -1));
    assert_eq!(
        one(&rig, "nope", 0, 0).await,
        (UNKNOWN_TOPIC_OR_PARTITION, -1)
    );
    // The refusals changed nothing.
    assert_eq!(rig.list_offset(7, "t", 0, -2).await.offset, 20);
}

#[tokio::test]
async fn a_topic_that_only_compacts_refuses_and_one_that_compacts_and_deletes_does_not() {
    let rig = rig();
    let compact = with_config(topic("c", 1), "cleanup.policy", "compact");
    let both = with_config(topic("b", 1), "cleanup.policy", "compact,delete");
    rig.create(vec![compact, both], false).await;
    for name in ["c", "b"] {
        let batch = support::keyed(&[("k", Some("v")); 10]);
        assert_eq!(rig.produce(-1, name, 0, batch).await.error_code, 0);
    }
    assert_eq!(one(&rig, "c", 0, 5).await, (POLICY_VIOLATION, -1));
    assert_eq!(one(&rig, "b", 0, 5).await, (0, 5));
}

#[tokio::test]
async fn a_topic_that_names_no_partition_has_no_entry_and_one_named_twice_is_answered_once_for_the_last()
 {
    let rig = rig();
    filled(&rig, 3).await;
    let r = delete(&rig, 2, &[("t", &[])]).await;
    assert!(r.topics.is_empty());
    let r = delete(&rig, 2, &[("t", &[(0, 5), (0, 12)])]).await;
    assert_eq!(r.topics[0].partitions.len(), 1);
    assert_eq!(r.topics[0].partitions[0].low_watermark, 12);
}

#[tokio::test]
async fn every_version_is_answered_in_a_form_that_can_be_written() {
    let rig = rig();
    filled(&rig, 2).await;
    for v in 0..=2 {
        let r = delete(&rig, v, &[("t", &[(0, 1), (9, 0)]), ("nope", &[(0, 0)])]).await;
        assert_eq!(r.topics.len(), 2, "v{v}");
    }
}

#[tokio::test]
async fn the_start_is_durable_and_survives_a_stop() {
    let rig = rig();
    filled(&rig, 4).await;
    assert_eq!(one(&rig, "t", 0, 17).await, (0, 17));
    let again = open_on(rig.fs.process_restart());
    assert_eq!(again.list_offset(7, "t", 0, -2).await.offset, 17);
    assert_eq!(again.list_offset(7, "t", 0, -1).await.offset, 40);
}

#[tokio::test]
async fn deleting_past_an_open_transactions_first_offset_holds_the_last_stable_offset_at_the_start()
{
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let t: Txn = rig.init_txn("tx", 60_000).await.unwrap();
    assert_eq!(rig.add_partitions("tx", t, "t", &[0]).await, [0]);
    assert_eq!(rig.produce_txn("tx", t, ("t", 0), 0, 3).await.error_code, 0);
    for _ in 0..4 {
        rig.produce(-1, "t", 0, records(10)).await;
    }
    assert_eq!(rig.fetch_committed("t", 0, 0).await.last_stable_offset, 0);
    // The start passes the open transaction's first record: nothing below the start can be the last stable offset.
    assert_eq!(one(&rig, "t", 0, 10).await, (0, 10));
    assert_eq!(rig.fetch_committed("t", 0, 10).await.last_stable_offset, 10);
    assert_eq!(rig.end_txn("tx", t, true).await, 0);
    assert!(rig.fetch_committed("t", 0, 10).await.last_stable_offset >= 10);
}
