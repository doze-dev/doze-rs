use super::*;

const NOW: i64 = 1_000_000;

fn write(p: &mut Producers, id: i64, epoch: i16, first: i32, count: u32, offset: i64) {
    let last = last_of(first, count);
    assert_eq!(
        p.check(id, epoch, first, last),
        Verdict::Accept,
        "{first}..={last}"
    );
    p.record(
        Stored {
            id,
            epoch,
            first,
            last,
            base_offset: offset,
            max_timestamp: 500 + offset,
        },
        NOW,
    );
}

#[test]
fn a_producer_nobody_knows_starts_wherever_it_is() {
    let p = Producers::default();
    assert_eq!(
        p.check(7, 3, 5, 5),
        Verdict::Accept,
        "recorded: unknown producer, sequence 5"
    );
}

#[test]
fn batches_in_order_are_accepted_and_a_repeat_is_answered_with_where_it_went() {
    let mut p = Producers::default();
    write(&mut p, 7, 0, 0, 3, 10);
    write(&mut p, 7, 0, 3, 2, 13);
    assert_eq!(
        p.check(7, 0, 0, 2),
        Verdict::Duplicate {
            offset: 10,
            timestamp: 510
        }
    );
    assert_eq!(
        p.check(7, 0, 3, 4),
        Verdict::Duplicate {
            offset: 13,
            timestamp: 513
        }
    );
}

#[test]
fn a_gap_and_an_overlap_are_out_of_order_and_one_from_long_ago_is_a_duplicate_nobody_can_place() {
    let mut p = Producers::default();
    for i in 0..7 {
        write(&mut p, 7, 0, i * 2, 2, i64::from(i) * 2);
    }
    // Seven batches of two written; the window keeps the last five (sequences 4 to 13).
    assert_eq!(
        p.check(7, 0, 20, 21),
        Verdict::OutOfOrder,
        "a gap: 14 is next"
    );
    assert_eq!(
        p.check(7, 0, 5, 6),
        Verdict::OutOfOrder,
        "overlaps two remembered batches"
    );
    assert_eq!(
        p.check(7, 0, 0, 1),
        Verdict::OutOfOrder,
        "older than all that is remembered: Kafka no longer has the batch, so it is out of order"
    );
    assert_eq!(p.check(7, 0, 14, 15), Verdict::Accept);
}

#[test]
fn an_older_epoch_is_fenced_and_a_newer_one_starts_at_zero() {
    let mut p = Producers::default();
    write(&mut p, 7, 2, 0, 1, 0);
    assert_eq!(p.check(7, 1, 1, 1), Verdict::Fenced);
    assert_eq!(p.check(7, 3, 4, 4), Verdict::OutOfOrder);
    assert_eq!(p.check(7, 3, 0, 0), Verdict::Accept);
    p.record(
        Stored {
            id: 7,
            epoch: 3,
            first: 0,
            last: 0,
            base_offset: 1,
            max_timestamp: 1,
        },
        NOW,
    );
    assert_eq!(
        p.check(7, 2, 1, 1),
        Verdict::Fenced,
        "the old epoch is gone for good"
    );
}

#[test]
fn sequences_wrap_at_thirty_one_bits() {
    let mut p = Producers::default();
    write(&mut p, 7, 0, i32::MAX - 1, 2, 0);
    assert_eq!(last_of(i32::MAX - 1, 2), i32::MAX);
    assert_eq!(
        p.check(7, 0, 0, 0),
        Verdict::Accept,
        "after the largest comes 0"
    );
    assert_eq!(last_of(i32::MAX, 3), 1, "a batch that crosses the wrap");
}

#[test]
fn producers_silent_for_a_day_are_forgotten_and_the_ceiling_holds() {
    let mut p = Producers::default();
    for id in 0..(SWEEP_EVERY as i64) {
        p.record(
            Stored {
                id,
                epoch: 0,
                first: 0,
                last: 0,
                base_offset: id,
                max_timestamp: id,
            },
            NOW - IDLE_MS - 1,
        );
    }
    // A thousand more, after the idle time: the table has grown by a thousand, so the sweep forgets those silent.
    for id in 0..(SWEEP_EVERY as i64) {
        p.record(
            Stored {
                id: 1_000_000 + id,
                epoch: 0,
                first: 0,
                last: 0,
                base_offset: 0,
                max_timestamp: 0,
            },
            NOW,
        );
    }
    assert_eq!(p.len(), SWEEP_EVERY);
    // A flood of active producers never takes the partition past its ceiling by more than a sweep.
    let mut q = Producers::default();
    for id in 0..(MOST as i64 + 2_500) {
        q.record(
            Stored {
                id,
                epoch: 0,
                first: 0,
                last: 0,
                base_offset: id,
                max_timestamp: id,
            },
            NOW + id,
        );
    }
    assert!(q.len() <= MOST + SWEEP_EVERY, "{}", q.len());
}

/// Random sequences: whatever happens, a batch accepted twice in a row at the same range is a duplicate the second
/// time, and nothing accepted is ever answered as new again.
#[test]
fn a_batch_that_was_written_is_never_written_again() {
    for seed in 1_u64..=200 {
        let mut state = seed;
        let mut rng = move |n: u64| {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            (z ^ (z >> 31)) % n
        };
        let mut p = Producers::default();
        let mut written: Vec<(i64, i16, i32, i32)> = Vec::new();
        let mut next_seq = [0_i32; 3];
        for step in 0..200 {
            let id = rng(3) as i64;
            let epoch = 0;
            let slot = usize::try_from(id).unwrap_or(0);
            let (first, count) = if rng(4) == 0 {
                // A retry of something recent, or garbage.
                let back = i32::try_from(rng(4)).unwrap_or(0);
                ((next_seq[slot] - 1 - back).max(0), 1)
            } else {
                (next_seq[slot], 1 + u32::try_from(rng(3)).unwrap_or(0))
            };
            let last = last_of(first, count);
            match p.check(id, epoch, first, last) {
                Verdict::Accept => {
                    assert!(
                        !written
                            .iter()
                            .any(|w| w.0 == id && w.2 == first && w.3 == last),
                        "seed {seed} step {step}: {first}..={last} of {id} written twice"
                    );
                    p.record(
                        Stored {
                            id,
                            epoch,
                            first,
                            last,
                            base_offset: step,
                            max_timestamp: step,
                        },
                        NOW,
                    );
                    written.push((id, epoch, first, last));
                    next_seq[slot] = next(last);
                }
                Verdict::Duplicate { .. } | Verdict::OutOfOrder | Verdict::Fenced => {}
            }
        }
    }
}

#[test]
fn a_marker_raises_the_producers_epoch_and_its_sequences_start_again() {
    let mut p = Producers::default();
    write(&mut p, 7, 0, 0, 3, 10);
    p.marked(7, 2, NOW);
    assert_eq!(p.snapshot()[0].1, 2, "the epoch the marker carried");
    assert_eq!(
        p.check(7, 1, 3, 3),
        Verdict::Fenced,
        "the old epoch is refused"
    );
    assert_eq!(
        p.check(7, 2, 5, 5),
        Verdict::OutOfOrder,
        "a new epoch starts at 0"
    );
    assert_eq!(p.check(7, 2, 0, 0), Verdict::Accept);
    // A marker at an epoch the producer is already at, or below, changes nothing.
    write(&mut p, 7, 2, 0, 1, 20);
    p.marked(7, 1, NOW);
    p.marked(7, 2, NOW);
    assert_eq!(p.check(7, 2, 1, 1), Verdict::Accept);
    assert_eq!(
        p.check(7, 2, 0, 0),
        Verdict::Duplicate {
            offset: 20,
            timestamp: 520
        }
    );
    // A producer never seen is known by its marker.
    p.marked(9, 4, NOW);
    assert_eq!(p.check(9, 3, 0, 0), Verdict::Fenced);
}

#[test]
fn a_producer_whose_batches_are_all_below_the_log_start_is_forgotten() {
    let mut p = Producers::default();
    write(&mut p, 7, 0, 0, 3, 10);
    write(&mut p, 8, 0, 0, 3, 40);
    p.marked(9, 1, NOW);
    p.forget_below(30);
    let mut left: Vec<i64> = p.snapshot().iter().map(|s| s.0).collect();
    left.sort_unstable();
    // 7 has nothing left in the log, so any sequence of it is a fresh start; 8 and the marker-only 9 are kept.
    assert_eq!(left, [8, 9]);
    assert_eq!(p.check(7, 0, 50, 50), Verdict::Accept);
    assert_eq!(
        p.check(8, 0, 0, 2),
        Verdict::Duplicate {
            offset: 40,
            timestamp: 540
        }
    );
}

#[test]
fn compaction_is_told_where_each_producers_newest_batch_ends_and_the_epoch_it_last_wrote_at() {
    let mut p = Producers::default();
    // Two batches in order: the second, newer one is where the producer's data ends (offset 13, 14: it has two records).
    write(&mut p, 7, 3, 0, 3, 10);
    write(&mut p, 7, 3, 3, 2, 13);
    // A batch that wraps the sequence space: four records, so its last offset is three above its first.
    write(&mut p, 8, 1, i32::MAX - 1, 4, 100);
    // Only a marker, at a later epoch.
    p.marked(9, 5, NOW);
    let lasts = p.lasts();
    let of = |id| lasts.get(&id).copied().unwrap();
    assert_eq!(of(7).data_last_offset, Some(14));
    assert_eq!(of(7).marker_epoch, 3);
    assert_eq!(of(8).data_last_offset, Some(103), "wrapped: 4 records");
    assert_eq!(of(8).marker_epoch, 1);
    assert_eq!(of(9).data_last_offset, None, "a marker is not data");
    assert_eq!(of(9).marker_epoch, 5);
}

#[test]
fn a_marker_read_back_with_nothing_before_it_leaves_the_sequences_open_until_a_marker_raises_the_epoch()
 {
    let mut p = Producers::default();
    // The batches the marker ended are gone from the log: the producer goes on at its epoch from wherever it is.
    p.marked_in_history(7, 3, NOW);
    assert_eq!(p.check(7, 2, 0, 0), Verdict::Fenced, "the epoch is known");
    assert_eq!(p.check(7, 3, 5, 5), Verdict::Accept);
    assert_eq!(
        p.check(7, 4, 5, 5),
        Verdict::OutOfOrder,
        "a higher epoch starts at 0"
    );
    // A marker found after data of the producer ended it: the data tells the sequence, the marker changes nothing.
    write(&mut p, 8, 3, 0, 2, 10);
    p.marked_in_history(8, 3, NOW);
    assert_eq!(p.check(8, 3, 2, 2), Verdict::Accept);
    assert_eq!(p.check(8, 3, 9, 9), Verdict::OutOfOrder);
    // A marker that raised the epoch above data read back starts the sequences at 0.
    p.marked_in_history(8, 4, NOW);
    assert_eq!(p.check(8, 4, 2, 2), Verdict::OutOfOrder);
    assert_eq!(p.check(8, 4, 0, 0), Verdict::Accept);
    // Likewise a later marker over an open start, and one the broker wrote itself, is not a loss of sequences.
    p.marked_in_history(7, 5, NOW);
    assert_eq!(p.check(7, 5, 8, 8), Verdict::OutOfOrder);
    p.marked(9, 3, NOW);
    assert_eq!(p.check(9, 3, 4, 4), Verdict::OutOfOrder);
}
