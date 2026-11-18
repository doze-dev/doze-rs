//! What is stored of a share-partition and what comes back from it.

use super::*;

#[test]
fn what_is_stored_is_the_start_and_every_record_not_simply_waiting_with_a_held_one_as_before_its_acquisition()
 {
    let mut p = SharePartition::new(0, 3);
    fetch(&mut p, "m", &[(0, 5)], 500, 0);
    p.acknowledge(
        "m",
        &[ack(
            0,
            5,
            &[ACCEPT, RELEASE, REJECT, ACCEPT, ACCEPT, ACCEPT],
        )],
        0,
        &LIMITS,
    )
    .unwrap();
    let stored = p.snapshot();
    assert_eq!(
        stored.start_offset, 0,
        "written before the start moves (Kafka's lag of one write)"
    );
    p.advance();
    assert_eq!(p.start, 1);
    // Offset 1 acquired again (count 2) is stored as it was before: available, delivered once.
    fetch(&mut p, "m", &[(0, 5)], 500, 0);
    let s = p.snapshot();
    let b = |first, last, state, delivery_count| ShareBatch {
        first,
        last,
        state,
        delivery_count,
    };
    assert_eq!(
        s,
        ShareState {
            state_epoch: 3,
            start_offset: 1,
            batches: vec![
                b(1, 1, ShareRecordState::Available, 1),
                b(2, 2, ShareRecordState::Archived, 1),
                b(3, 5, ShareRecordState::Acknowledged, 1),
            ],
        }
    );
}

#[test]
fn a_stored_state_comes_back_with_nothing_held_and_its_gaps_fetched_first() {
    let b = |first, last, state, delivery_count| ShareBatch {
        first,
        last,
        state,
        delivery_count,
    };
    let stored = ShareState {
        state_epoch: 1,
        start_offset: 10,
        batches: vec![
            b(12, 13, ShareRecordState::Acknowledged, 1),
            b(14, 14, ShareRecordState::Available, 2),
        ],
    };
    let mut p = SharePartition::restore(&stored).unwrap();
    assert_eq!((p.start, p.end), (10, 14));
    assert_eq!(p.next_fetch_offset(), 10, "the gap 10-11 first");
    let log = [(10, 11), (12, 13), (14, 14), (15, 16)];
    let got = fetch(&mut p, "m", &log, 500, 0);
    assert_eq!(ranges(&got), [(10, 11, 1), (14, 14, 3), (15, 16, 1)]);
    let bad = ShareState {
        start_offset: 13,
        ..stored
    };
    assert!(
        SharePartition::restore(&bad).is_err(),
        "a batch before the start"
    );
}
