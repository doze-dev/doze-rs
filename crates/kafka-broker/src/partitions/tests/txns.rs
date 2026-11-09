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
    assert_eq!(p.stability().last_stable.get(), 0, "producer 7 began at 0");
    assert_eq!(p.txns().await.unwrap().open.len(), 2);
    // 7 aborts and 8 commits.
    assert_eq!(append(&p, marker(7, Marker::Abort)).await, 3);
    assert_eq!(append(&p, marker(8, Marker::Commit)).await, 4);
    let seen = p.stability();
    assert_eq!(seen.last_stable.get(), 5, "nothing open: the end");
    assert_eq!(
        seen.aborted
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
    assert_eq!(q.stability().last_stable.get(), 5);
    assert_eq!(
        q.txns()
            .await
            .unwrap()
            .open
            .iter()
            .map(|o| o.producer)
            .collect::<Vec<_>>(),
        [9]
    );
    assert_eq!(q.stability().aborted.len(), 1);
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

/// A partition's reads of its log when it is opened again, in a world where it holds `transactional` batches or not.
async fn reads_on_reopening(transactional: bool) -> u64 {
    let (partitions, topic, meta, fs) = setup_full(|_| {}, 1);
    let id = PartitionId::new(0).unwrap();
    let p = partitions.get(&topic, id).await.unwrap();
    for _ in 0..40 {
        append(
            &p,
            Batch::parse(BatchBuilder::new().records(50).build()).unwrap(),
        )
        .await;
    }
    if transactional {
        append(&p, data(7, 0)).await;
        append(&p, marker(7, Marker::Commit)).await;
    }
    assert_eq!(
        p.stability().last_stable.get(),
        p.reader().end_offset().get()
    );
    drop(p);
    let disk = fs.process_restart();
    let before = disk.count(kafka_storage::sim::Op::Read);
    let restarted = Partitions::new(
        disk.clone(),
        Config::new("/data".into()),
        Default::default(),
        meta,
    );
    let q = restarted.get(&topic, id).await.unwrap();
    // Asking what a committed reader may read costs the log nothing, whatever it holds.
    let asked = disk.count(kafka_storage::sim::Op::Read);
    let _ = q.stability();
    assert_eq!(disk.count(kafka_storage::sim::Op::Read), asked);
    asked - before
}

#[tokio::test]
async fn a_partition_that_never_held_a_transaction_is_not_read_to_find_that_out() {
    let plain = reads_on_reopening(false).await;
    let with_transactions = reads_on_reopening(true).await;
    assert!(
        plain < with_transactions,
        "{plain} reads to reopen a log without transactions, {with_transactions} with them"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_transactional_batch_is_not_visible_to_a_reader_before_the_index_knows_it_is_open() {
    let (partitions, topic, _meta, _fs) = setup_full(|_| {}, 1);
    let id = PartitionId::new(0).unwrap();
    let p = std::sync::Arc::new(partitions.get(&topic, id).await.unwrap());
    // A reader on its own thread asks for the last stable offset as fast as it can, noting the end of the log it saw first.
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reader = {
        let (p, stop) = (std::sync::Arc::clone(&p), std::sync::Arc::clone(&stop));
        std::thread::spawn(move || {
            let mut seen = Vec::new();
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let before = p.reader().end_offset().get();
                let stable = p.stability().last_stable.get();
                seen.push((before, stable, p.reader().end_offset().get()));
            }
            seen
        })
    };
    // A producer writes a transaction's batch and then its marker, one after the other, so the batch at `at` is open exactly
    // while the end of the log is `at + 1`.
    let mut opened = Vec::new();
    for producer in 1..400 {
        opened.push(append(&p, data(producer, 0)).await);
        append(&p, marker(producer, Marker::Commit)).await;
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    for (before, stable, after) in reader.join().unwrap() {
        // The log showed the batch and, when the question was answered, not yet its marker: it was open throughout.
        if before == after && opened.contains(&(before - 1)) {
            assert!(
                stable < before,
                "the log showed an open batch at {} and the last stable offset was {stable}",
                before - 1
            );
        }
    }
}
