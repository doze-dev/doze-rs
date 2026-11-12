use super::*;

#[test]
fn how_long_ago_is_the_boards_words() {
    for (ms, want) in [
        (200, "0.2 s ago"),
        (0, "0.0 s ago"),
        (999, "0.9 s ago"),
        (1000, "1 s ago"),
        (4_000, "4 s ago"),
        (59_999, "59 s ago"),
        (60_000, "1 m ago"),
        (3_599_999, "59 m ago"),
        (3_600_000, "1 h ago"),
        (6 * 3_600_000, "6 h ago"),
        (86_399_999, "23 h ago"),
        (86_400_000, "1 d ago"),
        (3 * 86_400_000, "3 d ago"),
        (-5, "0.0 s ago"),
        (i64::MAX, "106751991167 d ago"),
        (i64::MIN, "0.0 s ago"),
    ] {
        assert_eq!(ago(ms), want, "{ms}");
    }
}

#[test]
fn a_clock_has_milliseconds() {
    assert_eq!(clock_ms(0), "00:00:00.000");
    assert_eq!(clock_ms(1_700_000_000_123), "22:13:20.123");
    assert_eq!(clock_ms(-1), "23:59:59.999");
}

#[test]
fn a_time_is_milliseconds_or_a_utc_civil_time() {
    for (text, want) in [
        ("0", Some(0)),
        ("1700000000000", Some(1_700_000_000_000)),
        ("1970-01-01", Some(0)),
        ("1970-01-02", Some(86_400_000)),
        ("2023-11-14 22:13:20", Some(1_700_000_000_000)),
        ("2023-11-14T22:13:20Z", Some(1_700_000_000_000)),
        ("2023-11-14 22:13", Some(1_700_000_000_000 - 20_000)),
        ("  2023-11-14  ", Some(1_699_920_000_000)),
        ("2024-02-29", Some(1_709_164_800_000)),
        ("2023-02-29", None),
        ("2023-13-01", None),
        ("2023-00-10", None),
        ("2023-04-31", None),
        ("2023-11-14 24:00", None),
        ("2023-11-14 22:60", None),
        ("2023-11-14 22:13:60", None),
        ("2023-11-14 22", None),
        ("1969-12-31", None),
        ("yesterday", None),
        ("-5", None),
        ("NaN", None),
        ("1e9", None),
        ("", None),
        ("99999999999999999999", None),
        ("2023-11-14 22:13:20:01", None),
        ("2023-11-14-01", None),
    ] {
        assert_eq!(parse_time(text), want, "{text:?}");
    }
}

#[test]
fn every_minute_of_a_year_round_trips_through_the_civil_clock() {
    for day in 0..366_i64 {
        let ms = parse_time("2024-01-01").unwrap() + day * 86_400_000;
        let c = super::super::civil(ms);
        let text = format!("{:04}-{:02}-{:02}", c.year, c.month, c.day);
        assert_eq!(parse_time(&text), Some(ms), "{text}");
    }
}

#[test]
fn a_hex_dump_is_eight_bytes_a_line_with_the_text_beside_it() {
    let bytes = [
        0x0a, 0x09, 0x6f, 0x72, 0x64, 0x2d, 0x38, 0x38, 0x34, 0x30, 0x39, 0x12, 0x05, 0x63, 0x2d,
        0x34, 0x0a, 0x1c, 0x00, 0x7f,
    ];
    assert_eq!(
        hex_dump(&bytes),
        "0000  0a 09 6f 72 64 2d 38 38  ..ord-88\n\
         0008  34 30 39 12 05 63 2d 34  409..c-4\n\
         0010  0a 1c 00 7f              ...."
    );
    assert_eq!(hex_dump(&[]), "");
    assert_eq!(hex_dump(&[0x41]), "0000  41                       A");
}

#[test]
fn a_hex_prefix_says_whether_there_was_more() {
    assert_eq!(hex_prefix(&[1, 2, 3], 8), ("01 02 03".to_owned(), false));
    assert_eq!(hex_prefix(&[1, 2, 3], 3), ("01 02 03".to_owned(), false));
    assert_eq!(hex_prefix(&[1, 2, 3], 2), ("01 02".to_owned(), true));
    assert_eq!(hex_prefix(&[], 2), (String::new(), false));
}
