//! Lag against ListOffsets and OffsetFetch answers: a table of the cases, and properties over many numbers.

use super::*;

#[test]
fn the_table_of_cases() {
    let table: [(i64, Option<i64>, Lag); 12] = [
        (138_402, Some(150_519), Lag::Behind(12_117)),
        (0, Some(10), Lag::Behind(10)),
        (9, Some(10), Lag::Behind(1)),
        (10, Some(10), Lag::CaughtUp),
        (0, Some(0), Lag::CaughtUp),
        // A topic recreated under an app that kept its position: the commit is beyond the new log.
        (16_200, Some(16_104), Lag::PastEnd(96)),
        (1, Some(0), Lag::PastEnd(1)),
        // Never committed: not lag `end`.
        (-1, Some(2_310), Lag::NoCommit),
        (-1, None, Lag::NoCommit),
        (-5, Some(1), Lag::NoCommit),
        // A partition that cannot be asked.
        (5, None, Lag::Gone),
        (i64::MAX, Some(i64::MIN), Lag::PastEnd(i64::MAX)),
    ];
    for (committed, end, want) in table {
        assert_eq!(
            lag(committed, end),
            want,
            "committed {committed}, end {end:?}"
        );
    }
}

/// A small deterministic generator for the property checks (no dependency).
fn numbers(seed: u64) -> impl Iterator<Item = i64> {
    let mut x = seed;
    std::iter::repeat_with(move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        i64::try_from(x % 2_000_000).unwrap_or(0) - 1_000
    })
}

#[test]
fn lag_is_never_negative_and_a_behind_group_is_exactly_the_difference() {
    let mut it = numbers(0x9e37_79b9_7f4a_7c15);
    for _ in 0..20_000 {
        let (committed, end) = (it.next().unwrap(), it.next().unwrap());
        let l = lag(committed, Some(end));
        match l {
            Lag::Behind(n) => {
                assert!(n >= 1);
                assert!(committed >= 0 && committed < end);
                assert_eq!(n, end - committed);
            }
            Lag::CaughtUp => assert_eq!(committed, end),
            Lag::PastEnd(n) => {
                assert!(n >= 1);
                assert_eq!(n, committed - end);
            }
            Lag::NoCommit => assert!(committed < 0),
            Lag::Gone => panic!("an end was given"),
        }
        assert!(l.behind() >= 0, "{committed} {end}");
    }
}
