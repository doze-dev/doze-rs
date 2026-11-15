//! Every batch the broker stores is written under leader epoch 0, as Kafka's leader stamps its epoch into each batch it appends
//! (`LogValidator`): a client's batch (written with -1), a transaction marker, a re-encoded batch. A log an older build wrote,
//! whose batches carry -1, is served as it is stored and read as epoch 0 (behaviour F11).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use bytes::Bytes;
use kafka_storage::fs::{Fs, FsFile};
use kafka_storage::sim::SimFs;
use kafka_wire::api::{Request, Response};
use kafka_wire::batch::{Batch, OneRecord, single_record};
use kafka_wire::messages::OffsetForLeaderEpochRequest;
use kafka_wire::messages::offset_for_leader_epoch_request::{
    OffsetForLeaderPartition, OffsetForLeaderTopic,
};
use support::{DATA, open_on, rig, topic, wire_name, with_config};

/// A batch as a client writes it: partition leader epoch -1.
fn client_batch(value: &str) -> Bytes {
    single_record(&OneRecord {
        timestamp_ms: 1_700_000_000_000,
        key: Some(b"k"),
        value: Some(value.as_bytes()),
        headers: &[],
    })
}

/// The partition leader epoch of each batch in `records`.
fn epochs(records: &Bytes) -> Vec<i32> {
    let mut out = Vec::new();
    let mut rest = records.clone();
    while rest.len() >= 61 {
        let len =
            usize::try_from(i32::from_be_bytes(rest[8..12].try_into().unwrap())).unwrap() + 12;
        let batch = rest.split_to(len);
        out.push(i32::from_be_bytes(batch[12..16].try_into().unwrap()));
        assert!(Batch::parse(batch).is_ok(), "the CRC still holds");
    }
    out
}

/// OffsetForLeaderEpoch v4 of `topic`'s partition 0 for epoch 0: (code, epoch, end offset).
async fn epoch_end(rig: &support::Rig, name: &str) -> (i16, i32, i64) {
    let mut p = OffsetForLeaderPartition::default();
    p.leader_epoch = 0;
    p.current_leader_epoch = 0;
    let mut t = OffsetForLeaderTopic::default();
    t.topic = wire_name(name);
    t.partitions.push(p);
    let mut req = OffsetForLeaderEpochRequest::default();
    req.topics.push(t);
    let Response::OffsetForLeaderEpoch(r) = rig.ask(4, Request::OffsetForLeaderEpoch(req)).await
    else {
        panic!("not OffsetForLeaderEpoch")
    };
    let e = &r.topics[0].partitions[0];
    (e.error_code, e.leader_epoch, e.end_offset)
}

#[tokio::test]
async fn a_clients_batch_is_stored_and_fetched_under_epoch_0() {
    let rig = rig();
    rig.create(vec![topic("orders", 1)], false).await;
    assert_eq!(
        rig.produce(-1, "orders", 0, client_batch("a"))
            .await
            .error_code,
        0
    );
    assert_eq!(
        rig.produce(1, "orders", 0, client_batch("b"))
            .await
            .error_code,
        0
    );
    let got = rig.fetch_now(&[("orders", 0, 0, 1 << 20)]).await;
    assert_eq!(epochs(got[0].records.as_ref().unwrap()), [0, 0]);
    assert_eq!(epoch_end(&rig, "orders").await, (0, 0, 2));
}

#[tokio::test]
async fn a_batch_the_topic_re_encodes_is_stored_under_epoch_0_too() {
    let rig = rig();
    rig.create(
        vec![with_config(topic("zipped", 1), "compression.type", "gzip")],
        false,
    )
    .await;
    assert_eq!(
        rig.produce(-1, "zipped", 0, client_batch("a"))
            .await
            .error_code,
        0
    );
    let got = rig.fetch_now(&[("zipped", 0, 0, 1 << 20)]).await;
    assert_eq!(epochs(got[0].records.as_ref().unwrap()), [0]);
}

#[tokio::test]
async fn a_log_an_older_build_wrote_with_minus_one_is_served_as_stored_and_read_as_epoch_0() {
    let fs = SimFs::new();
    fs.create_dir(std::path::Path::new("/")).ok();
    let rig = open_on(fs.clone());
    rig.create(vec![topic("old", 1)], false).await;
    // What a build before the stamp left on disk: two batches at offsets 0 and 1, each with epoch -1.
    let mut log = Vec::new();
    for (offset, value) in [(0_i64, "a"), (1, "b")] {
        let mut b = client_batch(value).to_vec();
        b[..8].copy_from_slice(&offset.to_be_bytes());
        log.extend_from_slice(&b);
    }
    let dir = std::path::Path::new(DATA).join("old-0");
    fs.create_dir_all(&dir).unwrap();
    let (f, _) = fs
        .open_or_create(&dir.join("00000000000000000000.log"))
        .unwrap();
    f.write_all_at(&log, 0).unwrap();
    let got = rig.fetch_now(&[("old", 0, 0, 1 << 20)]).await;
    assert_eq!(
        epochs(got[0].records.as_ref().unwrap()),
        [-1, -1],
        "as stored"
    );
    assert_eq!(
        epoch_end(&rig, "old").await,
        (0, 0, 2),
        "the whole log is epoch 0"
    );
    // What is written after it is stamped.
    assert_eq!(
        rig.produce(-1, "old", 0, client_batch("c"))
            .await
            .error_code,
        0
    );
    let got = rig.fetch_now(&[("old", 0, 2, 1 << 20)]).await;
    assert_eq!(
        epochs(got[0].records.as_ref().unwrap()),
        [0],
        "the new batch, from its own segment if the append rolled"
    );
    assert_eq!(epoch_end(&rig, "old").await, (0, 0, 3));
}
