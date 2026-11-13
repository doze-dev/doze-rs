#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use crate::civil::days_from_civil;
use crate::{Invalid, parse};

/// Milliseconds since the epoch at a UTC date and time.
fn at(y: i64, mo: u32, d: u32, h: i64, mi: i64) -> i64 {
    (days_from_civil(y, mo, d) * 1440 + h * 60 + mi) * 60_000
}

/// The next fire of `expr` after the given instant.
fn next(expr: &str, after: i64) -> Option<i64> {
    parse(expr).unwrap().next_after(0, after)
}

#[test]
fn a_rate_counts_from_its_anchor_and_never_replays() {
    let s = parse("rate(5 minutes)").unwrap();
    let anchor = at(2026, 1, 1, 0, 0);
    assert_eq!(s.next_after(anchor, anchor), Some(anchor + 300_000));
    assert_eq!(
        s.next_after(anchor, anchor + 300_000),
        Some(anchor + 600_000)
    );
    // A long sleep (a restart) asks for the next one after now; the missed ones are not owed.
    assert_eq!(
        s.next_after(anchor, anchor + 86_400_000 + 1),
        Some(anchor + 86_400_000 + 300_000)
    );
    assert_eq!(s.next_after(anchor, anchor - 10), Some(anchor + 300_000));
}

#[test]
fn rate_units_agree_with_their_number() {
    for good in [
        "rate(1 minute)",
        "rate(2 minutes)",
        "rate(1 hour)",
        "rate(12 hours)",
        "rate(1 day)",
    ] {
        assert!(parse(good).is_ok(), "{good}");
    }
    // The first two are what AWS refused when recorded (2026-11-09).
    for bad in [
        "rate(1 minutes)",
        "rate(5 minute)",
        "rate(0 minutes)",
        "rate(5 weeks)",
        "rate(-5 minutes)",
        "rate(5minutes)",
        "rate()",
        "rate(1.5 hours)",
        "rate(5 minutes",
        "rates(5 minutes)",
        "",
    ] {
        assert_eq!(parse(bad), Err(Invalid), "{bad}");
    }
}

#[test]
fn cron_fires_at_the_minute_and_not_before() {
    let from = at(2026, 11, 13, 10, 15);
    let e = "cron(15 10 * * ? *)";
    assert_eq!(next(e, from - 1), Some(from));
    assert_eq!(next(e, from), Some(at(2026, 11, 14, 10, 15)));
    assert_eq!(
        next("cron(0/15 * * * ? *)", from),
        Some(at(2026, 11, 13, 10, 30))
    );
    assert_eq!(next("cron(* * * * ? *)", from), Some(from + 60_000));
}

#[test]
fn lists_ranges_steps_and_names() {
    let from = at(2026, 11, 13, 0, 0);
    assert_eq!(
        next("cron(0 8,17 * * ? *)", from),
        Some(at(2026, 11, 13, 8, 0))
    );
    assert_eq!(
        next("cron(0 9-17/4 * * ? *)", at(2026, 11, 13, 9, 0)),
        Some(at(2026, 11, 13, 13, 0))
    );
    assert_eq!(
        next("cron(0 12 ? JAN-MAR MON *)", from),
        Some(at(2027, 1, 4, 12, 0))
    );
    assert_eq!(
        next("cron(0 12 ? * mon-fri *)", from),
        Some(at(2026, 11, 13, 12, 0))
    );
}

#[test]
fn weekdays_count_sunday_as_one() {
    // 2026-11-15 is a Sunday.
    let from = at(2026, 11, 13, 12, 0);
    assert_eq!(
        next("cron(0 12 ? * 1 *)", from),
        Some(at(2026, 11, 15, 12, 0))
    );
    assert_eq!(
        next("cron(0 12 ? * SUN *)", from),
        Some(at(2026, 11, 15, 12, 0))
    );
    assert_eq!(
        next("cron(0 12 ? * 7 *)", from),
        Some(at(2026, 11, 14, 12, 0))
    );
}

#[test]
fn last_day_nearest_weekday_and_nth_weekday() {
    let from = at(2026, 2, 1, 0, 0);
    assert_eq!(next("cron(0 0 L * ? *)", from), Some(at(2026, 2, 28, 0, 0)));
    assert_eq!(
        next("cron(0 0 L * ? *)", at(2028, 2, 1, 0, 0)),
        Some(at(2028, 2, 29, 0, 0))
    );
    // 2026-08-31 is a Monday; 2026-10-31 a Saturday, so the last weekday is Friday the 30th.
    assert_eq!(
        next("cron(0 0 LW 10 ? 2026)", from),
        Some(at(2026, 10, 30, 0, 0))
    );
    // 2026-11-15 is a Sunday: 15W is Monday the 16th. 2026-08-15 is a Saturday: Friday the 14th.
    assert_eq!(
        next("cron(0 0 15W 11 ? 2026)", from),
        Some(at(2026, 11, 16, 0, 0))
    );
    assert_eq!(
        next("cron(0 0 15W 8 ? 2026)", from),
        Some(at(2026, 8, 14, 0, 0))
    );
    // The first of a month on a Saturday moves to Monday the 3rd, not to the month before.
    assert_eq!(
        next("cron(0 0 1W 8 ? 2026)", from),
        Some(at(2026, 8, 3, 0, 0))
    );
    // The third Friday (6) of November 2026 is the 20th; the last Friday is the 27th.
    assert_eq!(
        next("cron(0 0 ? 11 6#3 2026)", from),
        Some(at(2026, 11, 20, 0, 0))
    );
    assert_eq!(
        next("cron(0 0 ? 11 6L 2026)", from),
        Some(at(2026, 11, 27, 0, 0))
    );
}

#[test]
fn a_date_that_never_comes_ends_the_schedule() {
    assert_eq!(next("cron(0 0 30 2 ? *)", 0), None);
    assert_eq!(next("cron(0 0 1 1 ? 2020)", at(2026, 1, 1, 0, 0)), None);
    assert_eq!(
        next("cron(0 0 29 2 ? *)", at(2026, 3, 1, 0, 0)),
        Some(at(2028, 2, 29, 0, 0))
    );
}

#[test]
fn what_aws_refuses_is_refused() {
    for bad in [
        "cron(0 0 * * * *)",
        "cron(0 0 ? * ? *)",
        "cron(0 0 * *)",
        "cron(60 0 * * ? *)",
        "cron(0 24 * * ? *)",
        "cron(0 0 32 * ? *)",
        "cron(0 0 * 13 ? *)",
        "cron(0 0 ? * 8 *)",
        "cron(0 0 ? * 1#6 *)",
        "cron(0 0 * * ? 1969)",
        "cron(0 0 * * ? 2200)",
        "cron(0 0 5-1 * ? *)",
        "cron(0/0 0 * * ? *)",
        "cron(0 0 ,5 * ? *)",
        "cron( 0 0 * * ? *)",
        "cron(0  0 * * ? *)",
        "cron(0 0 L ? ? *)",
    ] {
        assert_eq!(parse(bad), Err(Invalid), "{bad}");
    }
}
