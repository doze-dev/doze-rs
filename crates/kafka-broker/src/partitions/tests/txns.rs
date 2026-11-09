//! A partition's view of its transactions: from what is appended, and read back from the log after a restart.

use kafka_wire::batch::{Batch, Marker};
use kafka_wire::ids::{ProducerEpoch, ProducerId, TimestampMs};

use super::setup_full;
use super::*;

const TRANSACTIONAL: u16 = 0x10;

fn data(producer: i64, seq: i32) -> Batch {
    let bytes = BatchBuilder::new()
        .attributes(TRANSACTIONAL)
        .producer(producer, 0, seq)
        .build();
    Batch::parse(bytes).unwrap()
}

fn marker(producer: i64, kind: Marker) -> Batch {
    let bytes = kind.batch(
        ProducerId::new(producer).unwrap(),
        ProducerEpoch::from_wire(0),
        0,
        TimestampMs::from_wire(1),
    );
    Batch::parse(bytes).unwrap()
}

async fn append(p: &Partition<SimFs>, batch: Batch) -> i64 {
    let pending = p
        .submit(
            batch,
            TimestampMs::from_wire(1),
            TimestampType::Create,
            true,
        )
        .await
        .unwrap();
    match pending.await.unwrap().unwrap() {
        Written::Appended(a) => a.first_offset.get(),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn open_and_aborted_transactions_are_seen_as_they_are_appended_and_after_a_restart() {
    let (partitions, topic, meta, fs) = setup_full(|_| {}, 1);
    let id = PartitionId::new(0).unwrap();
    let p = partitions.get(&topic, id).await.unwrap();
    assert_eq!(append(&p, data(7, 0)).await, 0);
    assert_eq!(append(&p, data(8, 0)).await, 1);
    assert_eq!(
        append(&p, BatchBuilder::new().build().pipe_parse()).await,
        2,
        "a plain batch between"
    );
    let view = p.txns().await.unwrap();
    assert_eq!(view.last_stable.get(), 0, "producer 7 began at 0");
    assert_eq!(view.open.len(), 2);
    // 7 aborts and 8 commits.
    assert_eq!(append(&p, marker(7, Marker::Abort)).await, 3);
    assert_eq!(append(&p, marker(8, Marker::Commit)).await, 4);
    let view = p.txns().await.unwrap();
    assert_eq!(view.last_stable.get(), 5, "nothing open: the end");
    assert_eq!(
        view.aborted
            .iter()
            .map(|a| (a.producer, a.first, a.last))
            .collect::<Vec<_>>(),
        [(7, 0, 3)]
    );
    // A third is left open, and the process dies: the view is the same, read back from the log.
    assert_eq!(append(&p, data(9, 0)).await, 5);
    drop(p);
    let restarted = Partitions::new(
        fs.process_restart(),
        Config::new("/data".into()),
        Default::default(),
        meta,
    );
    let q = restarted.get(&topic, id).await.unwrap();
    let view = q.txns().await.unwrap();
    assert_eq!(view.last_stable.get(), 5);
    assert_eq!(
        view.open.iter().map(|o| o.producer).collect::<Vec<_>>(),
        [9]
    );
    assert_eq!(view.aborted.len(), 1);
}

/// `Bytes` of a batch as a `Batch`.
trait PipeParse {
    fn pipe_parse(self) -> Batch;
}

impl PipeParse for bytes::Bytes {
    fn pipe_parse(self) -> Batch {
        Batch::parse(self).unwrap()
    }
}
