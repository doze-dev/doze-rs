use super::*;

const P: Params = Params {
    min_dirty_ratio: 0.5,
    min_lag_ms: 0,
    max_lag_ms: i64::MAX,
    delete_retention_ms: 0,
    now_ms: 10_000,
    map_bytes: 1 << 20,
};

/// Segments from (base, size, newest timestamp), each ending where the next begins (the last, 100 on).
fn segs(parts: &[(i64, u64, i64)]) -> Vec<Facts> {
    parts
        .iter()
        .enumerate()
        .map(|(i, (base, size, newest_ms))| Facts {
            base: *base,
            next: parts.get(i + 1).map_or(base + 100, |n| n.0),
            size: *size,
            newest_ms: *newest_ms,
            oldest_ms: *newest_ms,
        })
        .collect()
}

#[test]
fn a_log_is_due_once_the_dirty_part_is_more_than_the_ratio_of_it_and_not_at_the_ratio() {
    // Cleaned to 100: 100 clean bytes, 100 dirty.
    let at_half = segs(&[(0, 100, 1), (100, 100, 2)]);
    assert_eq!(
        judge(&at_half, 100, 500, 200, &P),
        None,
        "exactly half is not more than half"
    );
    let over = segs(&[(0, 100, 1), (100, 101, 2)]);
    let plan = judge(&over, 100, 500, 200, &P).unwrap();
    assert_eq!(
        (
            plan.first_dirty,
            plan.upto,
            plan.dirty_bytes,
            plan.clean_bytes
        ),
        (100, 200, 101, 100)
    );
}

#[test]
fn nothing_dirty_is_nothing_to_do_and_a_log_never_cleaned_is_all_dirty() {
    let s = segs(&[(0, 100, 1), (100, 100, 2)]);
    assert_eq!(
        judge(&s, 200, 500, 200, &P),
        None,
        "cleaned to the active segment"
    );
    assert!(judge(&s, 0, 500, 200, &P).is_some());
    assert_eq!(judge(&[], 0, 0, 0, &P), None);
}

#[test]
fn the_segment_that_holds_the_first_dirty_offset_is_dirty_whole() {
    let s = segs(&[(0, 100, 1), (100, 100, 2), (200, 100, 3)]);
    let plan = judge(&s, 150, 1_000, 300, &P).unwrap();
    assert_eq!((plan.dirty_bytes, plan.clean_bytes), (200, 100));
}

#[test]
fn the_active_segment_and_a_transaction_still_open_limit_how_far_cleaning_goes() {
    let s = segs(&[(0, 100, 1), (100, 100, 2), (200, 100, 3)]);
    assert_eq!(judge(&s, 0, 1_000, 300, &P).unwrap().upto, 300);
    let plan = judge(&s, 0, 150, 300, &P).unwrap();
    assert_eq!(plan.upto, 150, "below the first open transaction");
    assert_eq!(
        plan.dirty_bytes, 200,
        "a segment the limit falls in is dirty whole"
    );
}

#[test]
fn the_minimum_lag_keeps_recent_segments_out_and_the_maximum_forces_an_old_dirty_part() {
    let lag = Params {
        min_lag_ms: 5_000,
        ..P
    };
    let s = segs(&[(0, 100, 1_000), (100, 100, 9_000), (200, 100, 9_500)]);
    assert_eq!(
        judge(&s, 0, 1_000, 300, &lag).unwrap().upto,
        100,
        "the second is newer than the lag"
    );
    // A dirty part below the ratio, but waiting longer than the maximum lag, is cleaned.
    let s = segs(&[(0, 1_000, 1_000), (1_000, 10, 2_000)]);
    assert_eq!(judge(&s, 1_000, 2_000, 1_010, &P), None);
    let forced = Params {
        max_lag_ms: 5_000,
        ..P
    };
    assert!(judge(&s, 1_000, 2_000, 1_010, &forced).is_some());
    let recent = Params {
        max_lag_ms: 9_000,
        ..P
    };
    assert!(
        judge(&s, 1_000, 2_000, 1_010, &recent).is_none(),
        "not yet old enough"
    );
}

/// A segment of `size` bytes holding offsets from `base`, first written at `oldest_ms` and last at `newest_ms`.
fn seg(base: i64, size: u64, oldest_ms: i64, newest_ms: i64) -> Facts {
    Facts {
        base,
        next: base + 10,
        size,
        newest_ms,
        oldest_ms,
    }
}

#[test]
fn a_record_stamped_in_the_future_blocks_nothing_when_there_is_no_minimum_lag() {
    let p = Params {
        min_dirty_ratio: 0.0,
        max_lag_ms: 86_400_000,
        ..P
    };
    // The first dirty segment holds a record a minute ahead of the clock (a client's): not "too new" with no lag to wait.
    let segments = [seg(0, 100, 1, 10_000 + 60_000), seg(10, 100, 1, 1)];
    assert!(judge(&segments, 0, 40, 30, &p).is_some());
}

#[test]
fn a_clean_segment_inside_the_minimum_lag_does_not_hold_back_the_dirty_ones_after_it() {
    let p = Params {
        min_dirty_ratio: 0.0,
        min_lag_ms: 600_000,
        now_ms: 1_000_000,
        ..P
    };
    // Cleaned to 10: the first segment is clean and young, the next two are dirty and old enough.
    let segments = [
        seg(0, 100, 900_000, 900_000),
        seg(10, 100, 100, 100),
        seg(20, 100, 100, 100),
    ];
    let plan = judge(&segments, 10, 40, 30, &p).unwrap();
    assert_eq!(plan.upto, 30);
    // A dirty one inside the lag does limit how far cleaning goes.
    let young = [
        seg(0, 100, 1, 1),
        seg(10, 100, 100, 100),
        seg(20, 100, 900_000, 900_000),
    ];
    assert_eq!(judge(&young, 10, 40, 30, &p).unwrap().upto, 20);
}

#[test]
fn the_maximum_lag_counts_a_segments_age_from_its_first_batch_and_the_earliest_of_the_dirty_ones_decides()
 {
    let p = Params {
        min_dirty_ratio: 1.0,
        max_lag_ms: 5_000,
        now_ms: 10_000,
        ..P
    };
    // Written from 100 to 9,900: its first batch is long past the lag, its last is not.
    let long_filling = [seg(0, 100, 100, 9_900), seg(10, 100, 9_950, 9_960)];
    assert!(judge(&long_filling, 0, 40, 20, &p).is_some());
    // The earliest of the dirty segments decides, not the first one.
    let later_older = [seg(0, 100, 9_000, 9_100), seg(10, 100, 100, 200)];
    assert!(judge(&later_older, 0, 40, 20, &p).is_some());
    // Every dirty segment began within the lag: not forced (a ratio of one is never passed).
    let young = [seg(0, 100, 6_000, 9_000), seg(10, 100, 9_000, 9_900)];
    assert!(judge(&young, 0, 40, 20, &p).is_none());
    // Exactly at the lag is not past it.
    let at = [seg(0, 100, 5_000, 5_000), seg(10, 100, 9_000, 9_900)];
    assert!(judge(&at, 0, 40, 20, &p).is_none());
    // A segment with no first batch to read (0) says nothing.
    let none = [seg(0, 100, 0, 0), seg(10, 100, 9_000, 9_900)];
    assert!(judge(&none, 0, 40, 20, &p).is_none());
    // A clean segment's age is no reason to clean.
    let clean_old = [seg(0, 100, 100, 100), seg(10, 100, 9_000, 9_900)];
    assert!(judge(&clean_old, 10, 40, 20, &p).is_none());
}

#[test]
fn the_minimum_lag_is_about_the_newest_record_and_the_edge_is_not_too_new() {
    let p = Params {
        min_dirty_ratio: 0.0,
        min_lag_ms: 1_000,
        now_ms: 10_000,
        ..P
    };
    // Exactly a lag old is old enough; a millisecond younger is too new.
    assert!(judge(&[seg(0, 100, 1, 9_000), seg(10, 1, 1, 1)], 0, 40, 20, &p).is_some());
    assert!(judge(&[seg(0, 100, 1, 9_001)], 0, 40, 10, &p).is_none());
}

#[test]
fn the_reading_says_what_stops_cleaning_and_agrees_with_the_verdict_whatever_the_log() {
    // Segments 0..100 (clean), 100..200, 200..300 (dirty), the active one from 300.
    let s = segs(&[(0, 100, 1), (100, 100, 2), (200, 100, 9_999)]);
    let r = reading(&s, 100, 1_000, 300, &P);
    assert_eq!(
        (r.limit, r.upto, r.first_dirty, r.dirty_start),
        (Limit::Active, 300, 100, 100)
    );
    assert_eq!((r.dirty_bytes, r.clean_bytes), (200, 100));
    // An open transaction at 250 stops it there.
    let r = reading(&s, 100, 250, 300, &P);
    assert_eq!((r.limit, r.upto), (Limit::Transaction, 250));
    // A minimum lag makes the newest dirty segment too new: cleaning stops at its base.
    let lagged = Params {
        min_lag_ms: 5_000,
        ..P
    };
    let r = reading(&s, 100, 1_000, 300, &lagged);
    assert_eq!((r.limit, r.upto), (Limit::TooNew, 200));
    assert_eq!(r.ratio(), Some(0.5));
    // The reading is the verdict's own numbers: due exactly when judge says so, with the same figures.
    for ratio in [0.0, 0.3, 0.5, 0.7] {
        let p = Params {
            min_dirty_ratio: ratio,
            ..P
        };
        let (r, plan) = (
            reading(&s, 100, 1_000, 300, &p),
            judge(&s, 100, 1_000, 300, &p),
        );
        assert_eq!(
            plan.is_some(),
            r.ratio().is_some_and(|x| x > ratio) || r.waited_too_long
        );
        if let Some(plan) = plan {
            assert_eq!(
                (plan.upto, plan.dirty_bytes, plan.clean_bytes),
                (r.upto, r.dirty_bytes, r.clean_bytes)
            );
        }
    }
    assert_eq!(
        reading(&[], 0, 0, 0, &P).ratio(),
        None,
        "nothing dirty has no share"
    );
}

#[test]
fn a_transaction_open_exactly_at_the_active_segment_is_the_active_segment_that_stops_cleaning() {
    let s = segs(&[(0, 100, 1), (100, 100, 2), (200, 100, 3)]);
    let r = reading(&s, 100, 300, 300, &P);
    assert_eq!(
        (r.limit, r.upto),
        (Limit::Active, 300),
        "a tie is the active segment's"
    );
    let r = reading(&s, 100, 299, 300, &P);
    assert_eq!((r.limit, r.upto), (Limit::Transaction, 299));
}
