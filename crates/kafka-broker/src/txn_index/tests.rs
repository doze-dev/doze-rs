use super::*;

#[test]
fn the_last_stable_offset_is_the_oldest_open_transactions_first_or_the_end() {
    let mut t = TxnIndex::default();
    assert_eq!(t.last_stable(10), 10, "nothing open: the end");
    t.write(7, 0, 4);
    t.write(7, 0, 5);
    t.write(8, 0, 6);
    assert_eq!(t.last_stable(10), 4, "producer 7's transaction began at 4");
    t.marker(7, 8, Marker::Commit);
    assert_eq!(t.last_stable(10), 6, "now producer 8's");
    t.marker(8, 9, Marker::Commit);
    assert_eq!(t.last_stable(10), 10);
}

#[test]
fn an_abort_is_remembered_until_the_log_start_passes_it_and_a_commit_is_not() {
    let mut t = TxnIndex::default();
    t.write(7, 0, 2);
    t.marker(7, 5, Marker::Abort);
    t.write(8, 0, 3);
    t.marker(8, 6, Marker::Commit);
    t.write(9, 0, 7);
    t.marker(9, 9, Marker::Abort);
    assert_eq!(
        t.aborted_between(0, 100),
        [
            Aborted {
                producer: 7,
                first: 2,
                last: 5
            },
            Aborted {
                producer: 9,
                first: 7,
                last: 9
            },
        ]
    );
    // Only those that touch the range: ended at or after it starts, begun before it ends.
    assert_eq!(t.aborted_between(6, 7), []);
    assert_eq!(t.aborted_between(6, 8).len(), 1);
    assert_eq!(
        t.aborted_between(5, 6)[0].producer,
        7,
        "the marker itself is in the range"
    );
    t.forget_before(6);
    assert_eq!(t.aborted_between(0, 100).len(), 1);
}

#[test]
fn a_marker_with_no_open_transaction_changes_nothing() {
    let mut t = TxnIndex::default();
    t.marker(7, 3, Marker::Abort);
    assert!(t.aborted_between(0, 10).is_empty());
    // Written again after a crash: the first closed it.
    t.write(7, 0, 4);
    t.marker(7, 5, Marker::Abort);
    t.marker(7, 6, Marker::Abort);
    assert_eq!(t.aborted_between(0, 10).len(), 1);
}

/// Whatever order the batches come in, a rebuild from the same batches is the same index.
#[test]
fn an_index_fed_batch_by_batch_is_the_index_rebuilt_from_them() {
    for seed in 1_u64..=100 {
        let mut state = seed;
        let mut rng = move |n: u64| {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            (z ^ (z >> 31)) % n
        };
        let mut log: Vec<(i64, Option<Marker>, i64)> = Vec::new();
        let mut live = TxnIndex::default();
        for offset in 0..60_i64 {
            let producer = i64::try_from(rng(4)).unwrap_or(0);
            let marker = (rng(4) == 0).then(|| {
                if rng(2) == 0 {
                    Marker::Commit
                } else {
                    Marker::Abort
                }
            });
            match marker {
                Some(kind) => live.marker(producer, offset, kind),
                None => live.write(producer, 0, offset),
            }
            log.push((producer, marker, offset));
        }
        let mut rebuilt = TxnIndex::default();
        for (producer, marker, offset) in log {
            match marker {
                Some(kind) => rebuilt.marker(producer, offset, kind),
                None => rebuilt.write(producer, 0, offset),
            }
        }
        assert_eq!(live.open(), rebuilt.open(), "seed {seed}");
        assert_eq!(
            live.aborted_between(0, 100),
            rebuilt.aborted_between(0, 100),
            "seed {seed}"
        );
        assert_eq!(live.last_stable(60), rebuilt.last_stable(60));
    }
}
