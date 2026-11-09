//! Compaction and producers: a producer's last batch is kept when its records are not, so that a retry after a restart is
//! recognised; an aborted transaction's records go; a transaction still open holds cleaning back.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use bytes::Bytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::batch::Batches;
use kafka_wire::batch::build::{BatchBuilder, Rec};
use kafka_wire::messages::TransactionalId;
use support::{keyed, open_on, produce_request, rig, topic, with_config};

fn compacted() -> kafka_wire::messages::create_topics_request::CreatableTopic {
    let t = with_config(topic("c", 1), "cleanup.policy", "compact");
    let t = with_config(t, "segment.bytes", "1024");
    with_config(t, "min.cleanable.dirty.ratio", "0.01")
}

/// A batch of these (key, value) records from a producer, first sequence `seq`, optionally in a transaction.
fn from(producer: i64, epoch: i16, seq: i32, transactional: bool, pairs: &[(&str, &str)]) -> Bytes {
    let recs: Vec<Rec> = pairs
        .iter()
        .map(|(k, v)| Rec {
            key: Some(k.as_bytes().to_vec()),
            value: Some(v.as_bytes().to_vec()),
            ..Rec::default()
        })
        .collect();
    BatchBuilder::from_records(1_000, &recs)
        .attributes(if transactional { 0x10 } else { 0 })
        .producer(producer, epoch, seq)
        .build()
}

/// The offsets and keys of the records the topic serves from the start.
async fn offsets_and_keys(rig: &support::Rig) -> Vec<(i64, String)> {
    let end = rig.list_offset(7, "c", 0, -1).await.offset;
    let (mut at, mut out) = (0, Vec::new());
    while at < end {
        let data = rig.fetch_now(&[("c", 0, at, 1 << 20)]).await.remove(0);
        for b in Batches::new(data.records.unwrap()) {
            let b = b.unwrap();
            b.each_record(1 << 20, |s| {
                if s.offset >= at && !b.attributes().is_control() {
                    out.push((
                        s.offset,
                        String::from_utf8(s.key.unwrap().to_vec()).unwrap(),
                    ));
                }
            })
            .unwrap();
            at = b.next_offset().unwrap().get();
        }
    }
    out
}

async fn produce_txn_batch(
    rig: &support::Rig,
    id: &str,
    batch: Bytes,
) -> kafka_wire::messages::produce_response::PartitionProduceResponse {
    let mut req = produce_request(-1, "c", &[(0, batch)]);
    req.transactional_id = Some(TransactionalId(kafka_wire::StrBytes::from_string(
        id.to_owned(),
    )));
    let Response::Produce(r) = rig.ask(9, Request::Produce(req)).await else {
        panic!("not Produce")
    };
    r.responses[0].partition_responses[0].clone()
}

async fn filler(rig: &support::Rig, n: usize) {
    for i in 0..n {
        let one = keyed(&[(&format!("f{}", i % 4), Some("x"))]);
        assert_eq!(rig.produce(-1, "c", 0, one).await.error_code, 0);
    }
}

#[tokio::test]
async fn a_producers_last_batch_is_kept_though_its_records_are_not_so_a_retry_after_a_restart_is_not_written_twice()
 {
    let rig = rig();
    rig.create(vec![compacted()], false).await;
    let p = rig.init_producer_id(None).await;
    let (id, epoch) = (p.producer_id.0, p.producer_epoch);
    // The idempotent producer writes key "a"; then someone else writes "a" again, so the producer's records are superseded.
    let first = rig
        .produce(-1, "c", 0, from(id, epoch, 0, false, &[("a", "1")]))
        .await;
    assert_eq!((first.error_code, first.base_offset), (0, 0));
    filler(&rig, 20).await;
    assert_eq!(
        rig.produce(-1, "c", 0, keyed(&[("a", Some("2"))]))
            .await
            .error_code,
        0
    );
    filler(&rig, 20).await;
    rig.broker.check_cleaner().await;
    assert!(!keys(&rig).await.is_empty());
    let end = rig.list_offset(7, "c", 0, -1).await.offset;
    let again = open_on(rig.fs.process_restart());
    // The producer, finding no answer, sends the same batch: it is recognised, not written.
    let retry = again
        .produce(-1, "c", 0, from(id, epoch, 0, false, &[("a", "1")]))
        .await;
    assert_eq!(
        (retry.error_code, retry.base_offset),
        (0, 0),
        "answered with where it went"
    );
    assert_eq!(
        again.list_offset(7, "c", 0, -1).await.offset,
        end,
        "and not written again"
    );
    let next = again
        .produce(-1, "c", 0, from(id, epoch, 1, false, &[("b", "1")]))
        .await;
    assert_eq!(next.error_code, 0, "the next in sequence is accepted");
}

#[tokio::test]
async fn the_records_of_an_aborted_transaction_are_cleaned_away_and_a_committed_ones_are_kept_if_latest()
 {
    let rig = rig();
    rig.create(vec![compacted()], false).await;
    let aborted = rig.init_txn("ta", 60_000).await.unwrap();
    assert_eq!(rig.add_partitions("ta", aborted, "c", &[0]).await, [0]);
    let b = from(
        aborted.producer,
        aborted.epoch,
        0,
        true,
        &[("lost", "1"), ("lost2", "1")],
    );
    assert_eq!(produce_txn_batch(&rig, "ta", b).await.error_code, 0);
    assert_eq!(rig.end_txn("ta", aborted, false).await, 0);
    let committed = rig.init_txn("tc", 60_000).await.unwrap();
    assert_eq!(rig.add_partitions("tc", committed, "c", &[0]).await, [0]);
    let b = from(
        committed.producer,
        committed.epoch,
        0,
        true,
        &[("kept", "1")],
    );
    assert_eq!(produce_txn_batch(&rig, "tc", b).await.error_code, 0);
    assert_eq!(rig.end_txn("tc", committed, true).await, 0);
    filler(&rig, 40).await;
    assert!(keys(&rig).await.contains(&"lost".to_owned()));
    rig.broker.check_cleaner().await;
    let after = keys(&rig).await;
    assert!(!after.iter().any(|k| k.starts_with("lost")), "{after:?}");
    assert!(after.contains(&"kept".to_owned()));
    // A read-committed consumer reads the topic through to its end all the same.
    let end = rig.list_offset(7, "c", 0, -1).await.offset;
    let data = rig.fetch_committed("c", 0, 0).await;
    assert_eq!((data.error_code, data.high_watermark), (0, end));
}

#[tokio::test]
async fn a_transaction_still_open_holds_cleaning_back_and_what_is_below_it_is_cleaned() {
    let rig = rig();
    rig.create(vec![compacted()], false).await;
    filler(&rig, 30).await;
    let t = rig.init_txn("open", 60_000).await.unwrap();
    assert_eq!(rig.add_partitions("open", t, "c", &[0]).await, [0]);
    let b = from(t.producer, t.epoch, 0, true, &[("f0", "tx"), ("f1", "tx")]);
    assert_eq!(produce_txn_batch(&rig, "open", b).await.error_code, 0);
    filler(&rig, 30).await;
    let before = keys(&rig).await.len();
    rig.broker.check_cleaner().await;
    let after = offsets_and_keys(&rig).await;
    assert!(
        after.len() < before,
        "what is below the transaction is cleaned"
    );
    // The transaction's records began at offset 30. Below it each key is there once; from it on nothing was touched.
    let (below, from): (Vec<_>, Vec<_>) = after.iter().partition(|(o, _)| *o < 30);
    let mut seen = std::collections::HashSet::new();
    assert!(
        below.iter().all(|(_, k)| seen.insert(k.clone())),
        "{below:?}"
    );
    assert_eq!(
        from.len(),
        32,
        "the transaction's two records and the thirty after: {from:?}"
    );
}

async fn keys(rig: &support::Rig) -> Vec<String> {
    offsets_and_keys(rig)
        .await
        .into_iter()
        .map(|(_, k)| k)
        .collect()
}

#[tokio::test]
async fn an_aborted_transactions_record_for_a_key_does_not_take_the_committed_value_with_it() {
    let rig = rig();
    rig.create(vec![compacted()], false).await;
    assert_eq!(
        rig.produce(-1, "c", 0, keyed(&[("k", Some("good"))]))
            .await
            .error_code,
        0
    );
    let aborted = rig.init_txn("ta", 60_000).await.unwrap();
    assert_eq!(rig.add_partitions("ta", aborted, "c", &[0]).await, [0]);
    let b = from(aborted.producer, aborted.epoch, 0, true, &[("k", "bad")]);
    assert_eq!(produce_txn_batch(&rig, "ta", b).await.error_code, 0);
    assert_eq!(rig.end_txn("ta", aborted, false).await, 0);
    filler(&rig, 40).await;
    rig.broker.check_cleaner().await;
    let after = keys(&rig).await;
    assert!(after.contains(&"k".to_owned()), "k is gone: {after:?}");
    let got = offsets_and_keys(&rig).await;
    assert_eq!(
        got.iter().filter(|(_, k)| k == "k").count(),
        1,
        "the aborted record is gone and the committed one stays: {got:?}"
    );
}
