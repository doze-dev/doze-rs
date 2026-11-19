//! The ordered JSON's tests.

use super::*;

#[test]
fn order_numbers_and_escapes_survive_a_round_trip() {
    let text = r#"{"b":1.50,"a":[true,null,-0e+3],"c":"x\"\\\n\u0001<\u00e9\ud83d\ude00"}"#;
    let v = Json::parse(text).unwrap();
    assert_eq!(
        v.to_text(),
        "{\"b\":1.50,\"a\":[true,null,-0e+3],\"c\":\"x\\\"\\\\\\n\\u0001<\u{e9}\u{1F600}\"}"
    );
    assert_eq!(v.get("b"), Some(&Json::Raw("1.50".into())));
}

#[test]
fn text_that_is_not_json_is_refused() {
    for bad in [
        "",
        "{",
        "[1,]",
        "{\"a\"}",
        "01",
        "1.",
        "-",
        "\"\\x\"",
        "tru",
        "{} x",
        "\"a\u{1}\"",
        "{'a':1}",
    ] {
        assert!(Json::parse(bad).is_err(), "{bad:?}");
    }
    assert!(Json::parse(&"[".repeat(2000)).is_err());
}

#[test]
fn depth_counts_nesting_outside_strings() {
    assert_eq!(depth(r#"{"a":[{"b":"[[[{"}]]}"#), 3);
    assert_eq!(depth(r#""\"[[""#), 0);
    assert_eq!(depth("[]"), 1);
}

#[test]
fn numbers_are_written_as_javas_big_decimal_writes_them() {
    // Recorded from cp-schema-registry 7.7.1 (tools/registry-diff): 1e2 is stored as 1E+2, 1.50 as itself.
    for (raw, want) in [
        ("1e2", "1E+2"),
        ("1.50", "1.50"),
        ("1", "1"),
        ("-0", "0"),
        ("-0.0", "0.0"),
        ("1.5e3", "1.5E+3"),
        ("1e-3", "0.001"),
        ("123e-2", "1.23"),
        ("0.0010", "0.0010"),
        ("1E-7", "1E-7"),
        ("-2.5E+10", "-2.5E+10"),
        ("10.0e1", "100"),
        ("true", "true"),
        ("0e0", "0"),
    ] {
        assert_eq!(super::java_number(raw), want, "{raw}");
    }
}

/// Found by fuzz json_schema: an exponent past i64 was read as 0, so `0.5e+<huge>` was stored as `0.5`, a different
/// number. One that can't be held, or whose scale can't, is kept as it was sent.
#[test]
fn a_number_too_large_to_rewrite_is_kept_as_sent() {
    for raw in [
        "0.555500000000000000005e+000011999153157185615783",
        "1e99999999999999999999",
        "-1.5e-99999999999999999999",
        "1.5e-9223372036854775808",
        "125e9223372036854775807",
    ] {
        assert_eq!(java_number(raw), raw, "{raw}");
    }
}
