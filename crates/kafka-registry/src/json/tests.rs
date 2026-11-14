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
