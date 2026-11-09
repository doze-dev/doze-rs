//! What the log's recent history says of its producers.

use bytes::Bytes;
use kafka_wire::batch::Batch;
use kafka_wire::batch::build::BatchBuilder;

use super::{NOW, fresh};
use kafka_wire::batch::TimestampType;

fn from(id: i64, epoch: i16, seq: i32, n: u32) -> Batch {
    let bytes: Bytes = BatchBuilder::new()
        .records(n)
        .producer(id, epoch, seq)
        .build();
    Batch::parse(bytes).unwrap()
}

#[test]
fn the_producers_batches_are_read_back_oldest_first_and_others_are_left_out() {
    let (_fs, _dir, mut log) = fresh(1 << 20);
    let plain = crate::testutil::batch(1, 5);
    log.append(&[from(7, 2, 0, 3)], NOW, TimestampType::Create)
        .unwrap();
    log.append(&[plain], NOW, TimestampType::Create).unwrap();
    log.append(
        &[from(8, 0, 10, 1), from(7, 2, 3, 2)],
        NOW,
        TimestampType::Create,
    )
    .unwrap();
    let history = log.reader().producer_history(1 << 20).unwrap();
    let seen: Vec<_> = history
        .iter()
        .map(|p| {
            let producer = p.producer.unwrap();
            (
                producer.id.get(),
                producer.epoch.get(),
                producer.base_sequence,
                p.base_offset,
                p.record_count,
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            (7, 2, Some(0), 0, 3),
            (8, 0, Some(10), 4, 1),
            (7, 2, Some(3), 5, 2),
        ]
    );
}

#[test]
fn only_the_last_segments_holding_enough_are_read() {
    // Segments of about one batch each: asking for little reads only the last.
    let (_fs, _dir, mut log) = fresh(1);
    for i in 0..6 {
        log.append(&[from(7, 0, i, 1)], NOW, TimestampType::Create)
            .unwrap();
    }
    let all = log.reader().producer_history(u64::MAX).unwrap();
    let little = log.reader().producer_history(1).unwrap();
    assert_eq!(all.len(), 6);
    assert!(
        !little.is_empty() && little.len() < all.len(),
        "{}",
        little.len()
    );
    assert_eq!(
        little.last().map(|p| p.base_offset),
        all.last().map(|p| p.base_offset),
        "the newest is always there"
    );
}
