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
