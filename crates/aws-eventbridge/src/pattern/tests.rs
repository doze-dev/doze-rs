#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use serde_json::{Value, json};

use super::{matches, parse};

fn event() -> Value {
    json!({
        "id": "1", "account": "123456789012", "source": "shop.orders",
        "time": "2026-01-01T00:00:00Z", "region": "us-east-1", "resources": [],
        "detail-type": "placed",
        "detail": {"total": 250, "customer": {"tier": "gold", "name": "Ada", "ip": "10.1.2.3"},
                   "items": ["book", "pen"], "note": null, "paid": true,
                   "lines": [{"sku": "a1", "qty": 2}, {"sku": "b2", "qty": 9}]}
    })
}

fn hit(pattern: &Value) -> bool {
    matches(&parse(&pattern.to_string()).unwrap(), &event())
}

/// The scenario `test_pattern_matching`, whose answers AWS recorded (2026-11-09).
#[test]
fn the_recorded_cases_answer_as_aws_did() {
    let recorded = [
        (json!({"source": ["shop.orders"]}), true),
        (json!({"source": ["shop.returns"]}), false),
        (json!({"source": [{"prefix": "shop."}]}), true),
        (json!({"source": [{"suffix": ".orders"}]}), true),
        (
            json!({"detail": {"customer": {"tier": [{"anything-but": ["bronze"]}]}}}),
            true,
        ),
        (
            json!({"detail": {"customer": {"tier": [{"anything-but": ["gold"]}]}}}),
            false,
        ),
        (
            json!({"detail": {"total": [{"numeric": [">=", 100, "<", 300]}]}}),
            true,
        ),
        (
            json!({"detail": {"total": [{"numeric": ["<", 100]}]}}),
            false,
        ),
        (
            json!({"detail": {"customer": {"name": [{"exists": true}]}}}),
            true,
        ),
        (json!({"detail": {"coupon": [{"exists": false}]}}), true),
        (json!({"detail": {"items": ["pen"]}}), true),
        (json!({"detail": {"note": [null]}}), true),
        (json!({"detail": {"paid": [true]}}), true),
        (
            json!({"detail": {"customer": {"name": [{"equals-ignore-case": "ADA"}]}}}),
            true,
        ),
        (json!({"source": [{"wildcard": "shop.*"}]}), true),
        (
            json!({"$or": [{"source": ["nope"]}, {"detail-type": ["placed"]}]}),
            true,
        ),
        (
            json!({"source": ["shop.orders"], "detail-type": ["refunded"]}),
            false,
        ),
    ];
    for (pattern, want) in recorded {
        assert_eq!(hit(&pattern), want, "{pattern}");
    }
}

#[test]
fn a_missing_field_matches_only_exists_false_and_so_do_its_parents() {
    assert!(!hit(&json!({"detail": {"coupon": ["x"]}})));
    assert!(!hit(&json!({"detail": {"coupon": [{"exists": true}]}})));
    assert!(!hit(
        &json!({"detail": {"coupon": [{"anything-but": ["x"]}]}})
    ));
    assert!(hit(
        &json!({"detail": {"coupon": [{"exists": false}, "x"]}})
    ));
    assert!(hit(
        &json!({"nothing": {"at": {"all": [{"exists": false}]}}})
    ));
    assert!(!hit(&json!({"detail": {"total": [{"exists": false}]}})));
}

#[test]
fn text_and_numbers_do_not_match_each_other() {
    assert!(hit(&json!({"detail": {"total": [250]}})));
    assert!(hit(&json!({"detail": {"total": [250.0]}})));
    assert!(!hit(&json!({"detail": {"total": ["250"]}})));
    assert!(!hit(&json!({"detail": {"customer": {"tier": [1]}}})));
    assert!(!hit(&json!({"detail": {"paid": ["true"]}})));
    assert!(!hit(&json!({"detail": {"note": [""]}})));
    assert!(!hit(&json!({"detail": {"total": [{"prefix": "2"}]}})));
}

#[test]
fn arrays_in_the_event_match_when_any_element_does() {
    assert!(hit(&json!({"detail": {"items": [{"prefix": "bo"}]}})));
    assert!(hit(&json!({"detail": {"items": ["zzz", "pen"]}})));
    assert!(!hit(&json!({"detail": {"items": ["zzz"]}})));
    // An array of objects: the nested pattern holds if one object satisfies all of it.
    assert!(hit(
        &json!({"detail": {"lines": {"sku": ["b2"], "qty": [{"numeric": [">", 5]}]}}})
    ));
    assert!(!hit(
        &json!({"detail": {"lines": {"sku": ["a1"], "qty": [{"numeric": [">", 5]}]}}})
    ));
    // An empty array exists, and has nothing to match.
    assert!(hit(&json!({"resources": [{"exists": true}]})));
    assert!(!hit(&json!({"resources": ["x"]})));
}

#[test]
fn the_string_conditions() {
    assert!(hit(
        &json!({"source": [{"prefix": {"equals-ignore-case": "SHOP."}}]})
    ));
    assert!(hit(
        &json!({"source": [{"suffix": {"equals-ignore-case": ".ORDERS"}}]})
    ));
    assert!(hit(&json!({"source": [{"wildcard": "*.orders"}]})));
    assert!(hit(&json!({"source": [{"wildcard": "s*p.*d*s"}]})));
    assert!(!hit(&json!({"source": [{"wildcard": "shop.*x"}]})));
    assert!(!hit(&json!({"source": [{"wildcard": "shop"}]})));
    assert!(hit(&json!({"source": [{"wildcard": "shop.orders"}]})));
    assert!(hit(&json!({"source": ["no", "shop.orders"]})));
    assert!(hit(
        &json!({"detail-type": [{"anything-but": {"prefix": "re"}}]})
    ));
    assert!(!hit(
        &json!({"detail-type": [{"anything-but": {"prefix": "pl"}}]})
    ));
    assert!(!hit(
        &json!({"detail-type": [{"anything-but": {"suffix": ["ed", "x"]}}]})
    ));
    assert!(hit(
        &json!({"detail-type": [{"anything-but": {"wildcard": "r*"}}]})
    ));
    assert!(hit(
        &json!({"detail-type": [{"anything-but": {"equals-ignore-case": "REFUNDED"}}]})
    ));
    assert!(hit(
        &json!({"detail": {"total": [{"anything-but": [1, 2]}]}})
    ));
    assert!(!hit(&json!({"detail": {"total": [{"anything-but": 250}]}})));
}

#[test]
fn a_wildcard_can_name_an_asterisk_and_a_backslash() {
    let e = json!({"x": "a*b", "y": "a\\b"});
    let m = |p: &str| {
        matches(
            &parse(&format!(r#"{{"x": [{{"wildcard": "{p}"}}]}}"#)).unwrap(),
            &e,
        )
    };
    assert!(m(r"a\\*b"));
    assert!(!m(r"a\\*c"));
    let y = |p: &str| {
        matches(
            &parse(&format!(r#"{{"y": [{{"wildcard": "{p}"}}]}}"#)).unwrap(),
            &e,
        )
    };
    assert!(y(r"a\\\\b"));
    assert!(y(r"a*b"));
}

#[test]
fn numbers_compare_as_numbers() {
    assert!(hit(
        &json!({"detail": {"total": [{"numeric": ["=", 250]}]}})
    ));
    assert!(hit(
        &json!({"detail": {"total": [{"numeric": [">", 249.5, "<=", 250]}]}})
    ));
    assert!(!hit(
        &json!({"detail": {"total": [{"numeric": [">", 250]}]}})
    ));
    assert!(hit(
        &json!({"detail": {"total": [{"numeric": ["<", 0]}, 250]}})
    ));
}

#[test]
fn cidr_covers_addresses_of_its_own_family() {
    let ip = |c: &str| hit(&json!({"detail": {"customer": {"ip": [{"cidr": c}]}}}));
    assert!(ip("10.1.0.0/16"));
    assert!(ip("10.1.2.3/32"));
    assert!(ip("0.0.0.0/0"));
    assert!(!ip("10.2.0.0/16"));
    assert!(!ip("::/0"));
    assert!(ip("10.0.0.0/7"));
    assert!(!ip("11.0.0.0/8"));
}

#[test]
fn or_may_nest_and_combine_with_other_keys() {
    assert!(hit(
        &json!({"source": ["shop.orders"], "$or": [{"detail-type": ["x"]}, {"detail": {"paid": [true]}}]})
    ));
    assert!(!hit(
        &json!({"source": ["shop.returns"], "$or": [{"detail-type": ["placed"]}]})
    ));
    assert!(hit(
        &json!({"detail": {"$or": [{"total": [1]}, {"customer": {"tier": ["gold"]}}]}})
    ));
    assert!(!hit(
        &json!({"$or": [{"source": ["a"]}, {"source": ["b"]}], "detail-type": ["placed"]})
    ));
}

/// The two refusals AWS worded when recorded (2026-11-09), to the character.
#[test]
fn the_recorded_refusals_are_worded_as_aws_worded_them() {
    let text = "{not json";
    let e = parse(text).unwrap_err();
    assert_eq!(
        e.message(text),
        "Event pattern is not valid. Reason: Unexpected character ('n' (code 110)): was expecting double-quote to start field name\n at [Source: (String)\"{not json\"; line: 1, column: 2]"
    );
    let text = r#"{"source": "x"}"#;
    let e = parse(text).unwrap_err();
    assert_eq!(
        e.message(text),
        "Event pattern is not valid. Reason: \"source\" must be an object or an array\n at [Source: (String)\"{\"source\": \"x\"}\"; line: 1, column: 13]"
    );
}

#[test]
fn what_is_not_a_pattern_is_refused() {
    for (bad, why) in [
        (r#"{"a": []}"#, "Empty arrays are not allowed"),
        (r#"{"a": [{"nope": 1}]}"#, "Unrecognized match type nope"),
        (
            r#"{"a": [{"exists": "yes"}]}"#,
            "exists match pattern must be either true or false.",
        ),
        (
            r#"{"a": [{"numeric": ["<>", 1]}]}"#,
            "Bad numeric range operator",
        ),
        (
            r#"{"a": [{"numeric": ["<", 1, ">", 0]}]}"#,
            "Bad numeric range operator",
        ),
        (
            r#"{"a": [{"numeric": [">", 5, "<", 1]}]}"#,
            "Bad numeric range operator",
        ),
        (
            r#"{"a": [[1]]}"#,
            "Match value must be String, number, true, false, or null",
        ),
        (
            r#"{"a": [{"prefix": 1}]}"#,
            "prefix/suffix match pattern must be a string",
        ),
        (
            r#"{"a": [{"cidr": "10.0.0.0"}]}"#,
            "Malformed CIDR, one '/' required",
        ),
        (
            r#"{"a": [{"cidr": "10.0.0.0/33"}]}"#,
            "Malformed CIDR, mask bits too large",
        ),
        (
            r#"{"a": [{"wildcard": "a**b"}]}"#,
            "Consecutive wildcard characters at pos 2",
        ),
        (r#"{"a": 5}"#, "\"a\" must be an object or an array"),
        (r#"{"$or": {}}"#, "$or value must be an array"),
        (r#"{"$or": [1]}"#, "$or must contain only objects"),
        (
            r#"{"a": [{"anything-but": [true]}]}"#,
            "Inside anything-but list, start|null|boolean is not supported.",
        ),
        (r#"[]"#, "Event pattern must be a JSON object"),
    ] {
        assert_eq!(parse(bad).unwrap_err().reason(), why, "{bad}");
    }
    for bad in [
        "",
        "{",
        "{\"a\"",
        "{\"a\": [1,}",
        "nul",
        "{\"a\": [\"\\x\"]}",
        "{\"a\": 1e999}",
        "{} x",
    ] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn a_pattern_cannot_overflow_the_stack() {
    let deep = format!(
        "{}{}",
        "{\"a\":".repeat(5000),
        "[1]".to_owned() + &"}".repeat(5000)
    );
    assert!(parse(&deep).is_err());
    let moderate = format!(
        "{}{}",
        "{\"a\":".repeat(20),
        "[1]".to_owned() + &"}".repeat(20)
    );
    assert!(parse(&moderate).is_ok());
}

#[test]
fn the_location_counts_lines_and_characters() {
    let text = "{\n  \"a\": 5\n}";
    let m = parse(text).unwrap_err().message(text);
    assert!(m.ends_with("line: 2, column: 9]"), "{m}");
}
