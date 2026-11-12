use super::*;

#[test]
fn a_topic_name_is_judged_by_the_brokers_own_rule_and_told_in_a_sentence() {
    let long = "a".repeat(250);
    let at_limit = "a".repeat(249);
    for (raw, want) in [
        ("orders", None),
        ("orders.retry", None),
        ("a_b-C.9", None),
        (at_limit.as_str(), None),
        ("", Some("A topic needs a name.")),
        (".", Some("'.' and '..' cannot be topic names.")),
        ("..", Some("'.' and '..' cannot be topic names.")),
        (
            long.as_str(),
            Some("250 characters is over the limit of 249."),
        ),
        (
            "a b",
            Some("U+0020 is not allowed: ASCII letters and digits, '.', '_' and '-' only."),
        ),
        (
            "a/b",
            Some("'/' is not allowed: ASCII letters and digits, '.', '_' and '-' only."),
        ),
        (
            "<script>",
            Some("'<' is not allowed: ASCII letters and digits, '.', '_' and '-' only."),
        ),
        (
            "a\0b",
            Some("U+0000 is not allowed: ASCII letters and digits, '.', '_' and '-' only."),
        ),
        (
            "caf\u{e9}",
            Some("'\u{e9}' is not allowed: ASCII letters and digits, '.', '_' and '-' only."),
        ),
        (
            "bad\u{fffd}",
            Some("U+FFFD is not allowed: ASCII letters and digits, '.', '_' and '-' only."),
        ),
    ] {
        assert_eq!(topic_name(raw).err().as_deref(), want, "{raw:?}");
    }
}

#[test]
fn every_name_the_broker_would_parse_passes_and_no_other_does() {
    // A sweep over short strings of the characters that matter: the console agrees with `TopicName::parse` on all.
    let alphabet = ['a', 'Z', '0', '.', '_', '-', ' ', '/', '\0', '\u{e9}', '<'];
    let mut seed = 7_u64;
    for _ in 0..4000 {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let len = (seed >> 60) as usize % 6;
        let raw: String = (0..len)
            .map(|i| alphabet[((seed >> (i * 5)) as usize) % alphabet.len()])
            .collect();
        assert_eq!(
            topic_name(&raw).is_ok(),
            TopicName::parse(&raw).is_ok(),
            "{raw:?}"
        );
    }
}

#[test]
fn a_name_is_taken_or_collides_or_is_free() {
    let there = ["orders", "orders.dlq", "inventory.levels"];
    assert_eq!(taken("orders", there), Taken::Exists);
    assert_eq!(taken("orders.retry", there), Taken::Free);
    assert_eq!(
        taken("orders_dlq", there),
        Taken::Collides("orders.dlq".to_owned())
    );
    assert_eq!(taken("Orders", there), Taken::Collides("orders".to_owned()));
    assert_eq!(
        taken("inventory_levels", there),
        Taken::Collides("inventory.levels".to_owned())
    );
    // An exact match wins whatever the order of the list.
    assert_eq!(taken("a.b", ["a_b", "a.b"]), Taken::Exists);
    assert_eq!(taken("x", [] as [&str; 0]), Taken::Free);
}

#[test]
fn partitions_are_whole_numbers_from_one_to_the_limit() {
    for (raw, most, want) in [
        ("3", Some(1000), Ok(3)),
        (" 3 ", Some(1000), Ok(3)),
        ("1", Some(1000), Ok(1)),
        ("1000", Some(1000), Ok(1000)),
        ("1001", None, Ok(1001)),
        ("0", Some(1000), Err("A topic has at least 1 partition.")),
        ("-1", Some(1000), Err("A topic has at least 1 partition.")),
        (
            "1001",
            Some(1000),
            Err("1,001 is above this broker's limit of 1,000 (--max-partitions)."),
        ),
        ("", None, Err("Type a whole number.")),
        ("three", None, Err("A whole number is needed, such as 3.")),
        ("3.5", None, Err("A whole number is needed, such as 3.")),
        (
            "99999999999",
            None,
            Err("That is more partitions than a number of this size can hold."),
        ),
    ] {
        assert_eq!(
            partitions(raw, most).map_err(|e| e.leak() as &str),
            want,
            "{raw:?}"
        );
    }
}

#[test]
fn retention_is_minus_one_or_zero_and_up_with_its_words() {
    assert_eq!(retention_ms("604800000"), Ok(604_800_000));
    assert_eq!(retention_ms("-1"), Ok(-1));
    assert_eq!(retention_ms("0"), Ok(0));
    assert!(retention_ms("-2").is_err());
    assert!(retention_ms("7d").is_err());
    assert_eq!(retention_words("604800000"), "= 7 d");
    assert_eq!(retention_words("3600000"), "= 1 h");
    assert_eq!(retention_words("-1"), "= no limit");
    assert_eq!(retention_words("0"), "= 0 s");
    assert_eq!(retention_words("abc"), "");
    assert_eq!(retention_words("-5"), "");
}

#[test]
fn the_offset_to_delete_before_is_minus_one_or_zero_and_up() {
    assert_eq!(delete_before("16000"), Ok(16_000));
    assert_eq!(delete_before("-1"), Ok(-1));
    assert_eq!(delete_before("0"), Ok(0));
    assert!(delete_before("-2").is_err());
    assert!(delete_before("").is_err());
    assert!(delete_before("1e3").is_err());
}

#[test]
fn acks_and_the_partition_choice() {
    assert_eq!(acks("-1"), Ok(-1));
    assert_eq!(acks("1"), Ok(1));
    assert_eq!(acks("0"), Ok(0));
    assert!(acks("2").is_err() && acks("all").is_err() && acks("").is_err());
    assert_eq!(partition_choice("", 3), Ok(None));
    assert_eq!(partition_choice("auto", 3), Ok(None));
    assert_eq!(partition_choice("2", 3), Ok(Some(2)));
    assert_eq!(partition_choice("0", 3), Ok(Some(0)));
    assert_eq!(
        partition_choice("3", 3),
        Err("This topic has partitions 0 to 2.".to_owned())
    );
    assert!(partition_choice("-1", 3).is_err());
    assert!(partition_choice("x", 3).is_err());
    assert!(partition_choice("99999999999", 3).is_err());
}

#[test]
fn nothing_panics_on_any_input() {
    for raw in [
        "",
        "\0",
        "\u{10ffff}",
        "-",
        "+",
        "--1",
        "9".repeat(400).as_str(),
    ] {
        let _ = (
            topic_name(raw),
            partitions(raw, Some(10)),
            retention_ms(raw),
            retention_words(raw),
            delete_before(raw),
            acks(raw),
            partition_choice(raw, 0),
        );
    }
}

#[test]
fn a_count_that_is_not_above_the_partitions_there_are_is_told_so() {
    assert_eq!(
        not_above(2, 3).unwrap(),
        "2 is below the 3 partitions there are."
    );
    assert_eq!(
        not_above(3, 3).unwrap(),
        "3 is the number of partitions there are already."
    );
    assert_eq!(not_above(4, 3), None);
    assert_eq!(not_above(1000, 1), None);
    assert_eq!(
        not_above(1, 1000).unwrap(),
        "1 is below the 1,000 partitions there are."
    );
}
