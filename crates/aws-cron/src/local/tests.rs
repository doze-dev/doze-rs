#![allow(clippy::unwrap_used)]

use crate::civil::days_from_civil;
use crate::{Invalid, Local, Zone, parse, parse_scheduler};

/// Milliseconds since the epoch at a UTC date and time.
fn utc(y: i64, mo: u32, d: u32, h: i64, mi: i64) -> i64 {
    (days_from_civil(y, mo, d) * 1440 + h * 60 + mi) * 60_000
}

const HOUR: i32 = 3600;

fn zone(name: &str) -> Zone {
    Zone::named(name).unwrap()
}

#[test]
fn zones_know_their_offsets_through_the_year() {
    let ny = zone("America/New_York");
    assert_eq!(ny.offset_at(utc(2026, 1, 15, 12, 0)), -5 * HOUR);
    assert_eq!(ny.offset_at(utc(2026, 7, 1, 12, 0)), -4 * HOUR);
    // 2026-03-08 02:00 EST is 07:00 UTC; 2026-11-01 02:00 EDT is 06:00 UTC.
    assert_eq!(ny.offset_at(utc(2026, 3, 8, 6, 59)), -5 * HOUR);
    assert_eq!(ny.offset_at(utc(2026, 3, 8, 7, 0)), -4 * HOUR);
    assert_eq!(ny.offset_at(utc(2026, 11, 1, 5, 59)), -4 * HOUR);
    assert_eq!(ny.offset_at(utc(2026, 11, 1, 6, 0)), -5 * HOUR);
    let sydney = zone("Australia/Sydney");
    assert_eq!(sydney.offset_at(utc(2026, 1, 10, 0, 0)), 11 * HOUR);
    assert_eq!(sydney.offset_at(utc(2026, 7, 10, 0, 0)), 10 * HOUR);
    // Dublin's rule has winter as its "daylight" time (a negative saving).
    let dublin = zone("Europe/Dublin");
    assert_eq!(dublin.offset_at(utc(2026, 1, 10, 0, 0)), 0);
    assert_eq!(dublin.offset_at(utc(2026, 7, 10, 0, 0)), HOUR);
    assert_eq!(zone("Asia/Kolkata").offset_at(0), 5 * HOUR + 1800);
    let howe = zone("Australia/Lord_Howe");
    assert_eq!(howe.offset_at(utc(2026, 1, 10, 0, 0)), 11 * HOUR);
    assert_eq!(howe.offset_at(utc(2026, 7, 10, 0, 0)), 10 * HOUR + 1800);
    assert_eq!(zone("UTC"), Zone::utc());
}

#[test]
fn unknown_zones_are_not_zones() {
    for name in [
        "Mars/Olympus",
        "america/new_york",
        "",
        "EST5EDT,M3.2.0,M11.1.0",
    ] {
        assert!(!Zone::known(name), "{name}");
    }
    assert!(Zone::known("Etc/GMT+5"));
}

#[test]
fn a_cron_time_the_clock_skips_does_not_fire_that_day() {
    // AWS's own example: cron(30 2 * * ? *) in America/Los_Angeles skips the spring-forward day.
    let s = parse_scheduler("cron(30 2 * * ? *)").unwrap();
    let la = zone("America/Los_Angeles");
    let after = utc(2026, 3, 7, 10, 31); // 02:31 PST on the 7th
    assert_eq!(
        s.next_in(&la, 0, after),
        Some(utc(2026, 3, 9, 9, 30)),
        "02:30 PDT on the 9th"
    );
}

#[test]
fn a_cron_time_the_clock_shows_twice_fires_once_at_the_second() {
    // 2026-11-01 in Los Angeles: 01:00-01:59 happens in PDT (08:xx UTC) and again in PST (09:xx UTC).
    let s = parse_scheduler("cron(30 1 * * ? *)").unwrap();
    let la = zone("America/Los_Angeles");
    let second = utc(2026, 11, 1, 9, 30);
    assert_eq!(s.next_in(&la, 0, utc(2026, 10, 31, 9, 0)), Some(second));
    assert_eq!(s.next_in(&la, 0, utc(2026, 11, 1, 8, 45)), Some(second));
    assert_eq!(s.next_in(&la, 0, second), Some(utc(2026, 11, 2, 9, 30)));
}

#[test]
fn an_every_minute_cron_crosses_both_changes_without_a_hole_or_a_repeat() {
    let s = parse_scheduler("cron(* * * * ? *)").unwrap();
    let ny = zone("America/New_York");
    for start in [utc(2026, 3, 8, 6, 0), utc(2026, 11, 1, 5, 0)] {
        let mut t = start;
        for _ in 0..180 {
            let n = s.next_in(&ny, 0, t).unwrap();
            assert!(n > t);
            // At fall back the first showing of the hour is skipped: one jump of an hour (or an hour and a minute).
            assert!(
                matches!(n - t, 60_000 | 3_600_000 | 3_660_000),
                "a gap of {} ms",
                n - t
            );
            t = n;
        }
    }
}

#[test]
fn a_daily_rate_is_a_day_of_real_time_across_a_change() {
    let s = parse_scheduler("rate(1 day)").unwrap();
    let anchor = utc(2026, 3, 7, 12, 0);
    let ny = zone("America/New_York");
    assert_eq!(s.next_in(&ny, anchor, anchor), Some(anchor + 86_400_000));
    assert_eq!(
        s.next_in(&ny, anchor, anchor + 86_400_000),
        Some(anchor + 2 * 86_400_000)
    );
}

#[test]
fn at_reads_a_wall_clock_reading_in_the_zone() {
    let s = parse_scheduler("at(2026-11-20T13:00:00)").unwrap();
    assert!(s.is_once());
    let la = zone("America/Los_Angeles");
    let t = utc(2026, 11, 20, 21, 0);
    assert_eq!(s.next_in(&la, 0, t - 1), Some(t));
    assert_eq!(s.next_in(&la, 0, t), None, "once only");
    assert_eq!(
        s.next_in(&Zone::utc(), 0, 0),
        Some(utc(2026, 11, 20, 13, 0))
    );
    let secs = parse_scheduler("at(2026-01-01T00:00:59)").unwrap();
    assert_eq!(
        secs.next_in(&Zone::utc(), 0, 0),
        Some(utc(2026, 1, 1, 0, 0) + 59_000)
    );
}

#[test]
fn at_in_a_gap_moves_on_and_in_a_fold_is_the_second() {
    let ny = zone("America/New_York");
    let gap = parse_scheduler("at(2026-03-08T02:30:00)").unwrap();
    assert_eq!(
        gap.next_in(&ny, 0, 0),
        Some(utc(2026, 3, 8, 7, 30)),
        "03:30 EDT"
    );
    let fold = parse_scheduler("at(2026-11-01T01:30:00)").unwrap();
    assert_eq!(
        fold.next_in(&ny, 0, 0),
        Some(utc(2026, 11, 1, 6, 30)),
        "01:30 EST"
    );
    let wall = utc(2026, 11, 1, 1, 30);
    assert_eq!(
        ny.resolve(wall),
        Local::Fold(utc(2026, 11, 1, 5, 30), utc(2026, 11, 1, 6, 30))
    );
}

#[test]
fn malformed_at_expressions_are_refused() {
    for text in [
        "at(2026-11-20T13:00)",
        "at(2026-11-20 13:00:00)",
        "at(2026-02-29T00:00:00)",
        "at(2026-13-01T00:00:00)",
        "at(2026-11-20T24:00:00)",
        "at(2026-11-20T13:00:00Z)",
        "at()",
    ] {
        assert_eq!(parse_scheduler(text), Err(Invalid), "{text}");
    }
    assert!(parse_scheduler("at(2028-02-29T23:59:59)").is_ok());
    // EventBridge rules do not take at().
    assert_eq!(parse("at(2026-11-20T13:00:00)"), Err(Invalid));
}
