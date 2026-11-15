//! `ConsoleView::producers`: the window of batches a partition keeps for each producer, and what an open transaction holds
//! back, held to what DescribeProducers and the log say.
#![cfg(feature = "inspect")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_auth::Principal;
use kafka_broker::Caller;
use kafka_wire::batch::build::BatchBuilder;
use support::{rig, topic};

fn console() -> Caller {
    Caller::new(Principal::console(), Some("127.0.0.1".parse().unwrap()))
}

fn idempotent(producer: i64, epoch: i16, seq: i32, n: u32) -> bytes::Bytes {
    BatchBuilder::new()
        .records(n)
        .timestamps(1_000, 1_000)
        .producer(producer, epoch, seq)
        .build()
}

/// CON-510: a producer's window is its last five batches, newest first, with their sequences and where they went.
#[tokio::test]
async fn each_producer_has_its_last_five_batches_newest_first() {
    let rig = rig();
    rig.create(vec![topic("p", 1)], false).await;
    let p = rig.init_producer_id(None).await;
    let (id, epoch) = (p.producer_id.0, p.producer_epoch);
    let mut seq = 0;
    for n in [4, 2, 1, 3, 2, 5, 4] {
        assert_eq!(
            rig.produce(-1, "p", 0, idempotent(id, epoch, seq, n))
                .await
                .error_code,
            0
        );
        seq += i32::try_from(n).unwrap();
    }
    let other = rig.init_producer_id(None).await;
    rig.produce(
        -1,
        "p",
        0,
        idempotent(other.producer_id.0, other.producer_epoch, 0, 2),
    )
    .await;
    let view = rig
        .broker
        .console_view()
        .producers(&console(), "p", 0)
        .await
        .unwrap();
    assert_eq!((view.start, view.end, view.last_stable), (0, 23, 23));
    assert_eq!(view.producers.len(), 2);
    let ours = view.producers.iter().find(|w| w.id == id).unwrap();
    assert_eq!((ours.epoch, ours.last_sequence), (epoch, Some(20)));
    assert_eq!(ours.batches.len(), 5, "five, not seven");
    // Newest first: the last batch had 4 records, sequences 17 to 20, at offsets 17 to 20 (the other producer wrote 21 and 22).
    let seqs: Vec<(i32, i32)> = ours
        .batches
        .iter()
        .map(|b| (b.first_sequence, b.last_sequence))
        .collect();
    assert_eq!(seqs, [(17, 20), (12, 16), (10, 11), (7, 9), (6, 6)]);
    let offs: Vec<(i64, i64)> = ours
        .batches
        .iter()
        .map(|b| (b.first_offset, b.last_offset))
        .collect();
    assert_eq!(
        offs,
        seqs.iter()
            .map(|(a, b)| (i64::from(*a), i64::from(*b)))
            .collect::<Vec<_>>(),
        "the producer wrote first, so offsets equal sequences"
    );
    assert!(ours.open_from.is_none());
}

/// CON-511: an open transaction holds the last stable offset where it began; an abort is remembered with its marker.
#[tokio::test]
async fn an_open_transaction_holds_the_last_stable_offset_and_an_abort_is_remembered() {
    let rig = rig();
    rig.create(vec![topic("p", 1)], false).await;
    rig.produce(1, "p", 0, support::records(3)).await;
    let t = rig.init_txn("tx", 60_000).await.unwrap();
    assert_eq!(rig.add_partitions("tx", t, "p", &[0]).await, [0]);
    assert_eq!(rig.produce_txn("tx", t, ("p", 0), 0, 2).await.error_code, 0);
    rig.produce(1, "p", 0, support::records(1)).await;
    let view = rig
        .broker
        .console_view()
        .producers(&console(), "p", 0)
        .await
        .unwrap();
    assert_eq!(
        (view.end, view.last_stable),
        (6, 3),
        "held back at where the transaction began"
    );
    let w = view.producers.iter().find(|w| w.id == t.producer).unwrap();
    assert_eq!(w.open_from, Some(3));
    assert!(view.aborted.is_empty());
    assert_eq!(rig.end_txn("tx", t, false).await, 0);
    let view = rig
        .broker
        .console_view()
        .producers(&console(), "p", 0)
        .await
        .unwrap();
    assert_eq!(view.last_stable, view.end, "the abort released it");
    let [a] = view.aborted.as_slice() else {
        panic!("{:?}", view.aborted);
    };
    assert_eq!((a.producer, a.first, a.marker), (t.producer, 3, 6));
    let w = view.producers.iter().find(|w| w.id == t.producer).unwrap();
    assert_eq!(w.open_from, None);
}

/// CON-512: looking leaves nothing behind and writes nothing: the log end and the open set are as they were.
#[tokio::test]
async fn looking_writes_nothing_and_keeps_nothing() {
    let rig = rig();
    rig.create(vec![topic("p", 2)], false).await;
    rig.produce(1, "p", 0, support::records(3)).await;
    let end = rig.list_offset(7, "p", 0, -1).await.offset;
    let open = rig.broker.open_partitions();
    for _ in 0..3 {
        let v = rig
            .broker
            .console_view()
            .producers(&console(), "p", 0)
            .await
            .unwrap();
        assert!(v.producers.is_empty() && v.aborted.is_empty());
        let _ = rig
            .broker
            .console_view()
            .producers(&console(), "p", 1)
            .await
            .unwrap();
    }
    assert_eq!(rig.list_offset(7, "p", 0, -1).await.offset, end);
    assert_eq!(
        rig.broker.open_partitions(),
        open,
        "partition 1 was opened to be looked at and let go again"
    );
}
