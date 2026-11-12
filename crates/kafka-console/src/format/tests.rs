use super::*;

#[test]
fn a_span_is_its_two_largest_units() {
    for (secs, want) in [
        (-5, "0 s"),
        (0, "0 s"),
        (1, "1 s"),
        (41, "41 s"),
        (59, "59 s"),
        (60, "1 min"),
        (119, "1 min"),
        (3599, "59 min"),
        (3600, "1 h"),
        (3600 + 5 * 60 + 59, "1 h 5 min"),
        (86_399, "23 h 59 min"),
        (86_400, "1 d"),
        (3 * 86_400 + 4 * 3600 + 59, "3 d 4 h"),
        (3 * 86_400 + 3599, "3 d"),
    ] {
        assert_eq!(span(secs), want, "{secs}");
    }
    assert_eq!(span_ms(41_999), "41 s");
}

#[test]
fn a_longer_wait_never_reads_as_a_shorter_one() {
    // The unit may change, the order of the instants may not: compare by re-parsing the two numbers.
    let weight = |s: &str| {
        let mut parts = s.split_whitespace();
        let mut total = 0_i64;
        while let (Some(n), Some(unit)) = (parts.next(), parts.next()) {
            let n: i64 = n.parse().unwrap();
            total += n * match unit {
                "s" => 1,
                "min" => 60,
                "h" => 3600,
                "d" => 86_400,
                other => panic!("{other}"),
            };
        }
        total
    };
    let mut last = 0;
    for secs in (0..400_000).step_by(7) {
        let w = weight(&span(secs));
        assert!(w >= last && w <= secs, "{secs}: {w}");
        last = w;
    }
}

#[test]
fn a_number_has_thousands_separators_and_a_sign() {
    for (n, want) in [
        (0, "0"),
        (7, "7"),
        (999, "999"),
        (1000, "1,000"),
        (16_104, "16,104"),
        (1_234_567, "1,234,567"),
        (-1_000, "-1,000"),
        (-999, "-999"),
        (i64::MIN, "-9,223,372,036,854,775,808"),
        (i64::MAX, "9,223,372,036,854,775,807"),
    ] {
        assert_eq!(number(n), want, "{n}");
    }
}

#[test]
fn a_number_without_its_commas_is_the_number() {
    for n in [
        -5_000_000_001_i64,
        -1,
        0,
        1,
        10,
        100,
        1_000,
        99_999,
        100_000,
        4_294_967_296,
    ] {
        assert_eq!(number(n).replace(',', "").parse::<i64>().unwrap(), n);
    }
}

#[test]
fn a_size_is_binary_with_one_decimal() {
    for (n, want) in [
        (0, "0 B"),
        (1023, "1023 B"),
        (1024, "1.0 KiB"),
        (1536, "1.5 KiB"),
        (1024 * 1024 - 1, "1023.9 KiB"),
        (1024 * 1024, "1.0 MiB"),
        (4_718_592, "4.5 MiB"),
        (4_301_200, "4.1 MiB"),
        (1 << 30, "1.0 GiB"),
        (1 << 40, "1.0 TiB"),
        (u64::MAX, "16777215.9 TiB"),
    ] {
        assert_eq!(bytes(n), want, "{n}");
    }
}

#[test]
fn milliseconds_turn_to_seconds_at_a_thousand() {
    assert_eq!(millis(0), "0 ms");
    assert_eq!(millis(2), "2 ms");
    assert_eq!(millis(999), "999 ms");
    assert_eq!(millis(1000), "1.0 s");
    assert_eq!(millis(1450), "1.4 s");
}

#[test]
fn a_count_agrees_with_its_number() {
    assert_eq!(count(0, "topic", "topics"), "0 topics");
    assert_eq!(count(1, "topic", "topics"), "1 topic");
    assert_eq!(count(2, "topic", "topics"), "2 topics");
    assert_eq!(count(1200, "topic", "topics"), "1,200 topics");
}

#[test]
fn civil_time_matches_known_instants() {
    // 2026-11-06 18:02:07 UTC, and the epoch, and a leap day, and a negative instant.
    let c = civil(1_793_988_127_000);
    assert_eq!(
        (c.year, c.month, c.day, c.hour, c.minute, c.second),
        (2026, 11, 6, 18, 2, 7)
    );
    assert_eq!(civil(0).year, 1970);
    let c = civil(951_782_400_000);
    assert_eq!((c.year, c.month, c.day), (2000, 2, 29));
    let c = civil(-1);
    assert_eq!(
        (c.year, c.month, c.day, c.hour, c.minute, c.second),
        (1969, 12, 31, 23, 59, 59)
    );
    assert_eq!(date_time(1_793_988_127_000), "Nov 6 18:02 UTC");
    assert_eq!(clock(1_793_988_127_000), "18:02:07");
}

#[test]
fn every_day_for_two_hundred_years_has_a_valid_date_and_follows_the_one_before() {
    let mut last = civil(0);
    for day in 1..73_000_i64 {
        let c = civil(day * 86_400_000 + 3_723_000);
        assert!(
            (1..=12).contains(&c.month) && (1..=31).contains(&c.day),
            "{day}"
        );
        assert_eq!((c.hour, c.minute, c.second), (1, 2, 3));
        let next_day = (c.year, c.month, c.day) > (last.year, last.month, last.day);
        assert!(next_day, "{day}: {c:?} after {last:?}");
        last = c;
    }
}
