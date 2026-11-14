#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;

fn m(pattern: &str, message: &str) -> bool {
    parse(pattern).unwrap().matches(message)
}

#[test]
fn the_empty_pattern_selects_everything() {
    assert!(m("", "anything"));
    assert!(m("   ", ""));
}

#[test]
fn terms_are_anded_phrases_are_kept_whole_and_a_minus_excludes() {
    // The four recorded selections (test_events_are_written_read_and_filtered).
    let lines = [
        "INFO started",
        "ERROR disk full",
        r#"{"level":"error","code":507}"#,
        "INFO stopped",
    ];
    let pick = |p: &str| -> Vec<&str> { lines.iter().copied().filter(|l| m(p, l)).collect() };
    assert_eq!(pick("ERROR"), ["ERROR disk full"]);
    assert_eq!(pick("INFO started"), ["INFO started"]);
    assert_eq!(pick(r#""disk full""#), ["ERROR disk full"]);
    assert_eq!(pick("INFO -started"), ["INFO stopped"]);
    assert_eq!(pick("?started ?stopped"), ["INFO started", "INFO stopped"]);
    // Case matters.
    assert!(pick("error disk").is_empty());
}

#[test]
fn a_term_is_a_whole_word_and_a_star_matches_a_run() {
    assert!(!m("Exception", "java.lang.RuntimeException: boom"));
    assert!(m("RuntimeException", "java.lang.RuntimeException: boom"));
    assert!(m("ERR*", "ERROR happened"));
    assert!(m("ERR*", "an ERROR happened"));
    assert!(!m("ERR*", "an error happened"));
    assert!(!m("disk", "diskette"));
}

#[test]
fn json_patterns_compare_numbers_strings_and_combine() {
    let msg = r#"{"level":"error","code":507,"user":{"id":"u1"},"tags":["a","b"],"gone":null}"#;
    for yes in [
        "{ $.code = 507 }",
        r#"{ $.level = "error" }"#,
        r#"{ $.level = "err*" }"#,
        "{ $.code >= 500 && $.code < 600 }",
        r#"{ $.user.id = "u1" }"#,
        r#"{ $.tags[0] = "a" }"#,
        r#"{ $.tags[*] = "b" }"#,
        "{ $.code != 200 }",
        "{ $.gone IS NULL }",
        "{ $.code EXISTS }",
        "{ $.nope NOT EXISTS }",
        r#"{ ($.code = 1) || ($.level = "error") }"#,
    ] {
        assert!(m(yes, msg), "{yes}");
    }
    for no in [
        "{ $.code = 200 }",
        "{ $.code > 507 }",
        r#"{ $.level = "warn" }"#,
        "{ $.nope = 1 }",
        "{ $.nope != 1 }",
        "{ $.code = 507 && $.level = \"info\" }",
    ] {
        assert!(!m(no, msg), "{no}");
    }
    // Not JSON: not selected.
    assert!(!m("{ $.code = 507 }", "code 507"));
}

#[test]
fn bad_json_patterns_are_invalid() {
    // Recorded: `{ $.code = }` is "Invalid filter pattern".
    for bad in [
        "{ $.code = }",
        "{ $.code 5 }",
        "{ $.code = 5",
        "{ code = 5 }",
        "{ ($.a = 1 }",
        "{ $.a = 1 & $.b = 2 }",
    ] {
        assert_eq!(parse(bad).unwrap_err(), PatternError::Invalid, "{bad}");
    }
}

#[test]
fn space_delimited_patterns_name_fields_and_test_them() {
    let line = r#"127.0.0.1 - frank [10/Oct/2000:13:55:36] "GET /a.gif HTTP/1.0" 404 2326"#;
    let p = parse(r#"[ip, user, username, timestamp, request, status_code = 404, bytes]"#).unwrap();
    assert!(p.matches(line));
    assert_eq!(p.pick(line, "$bytes").as_deref(), Some("2326"));
    assert_eq!(
        p.pick(line, "$request").as_deref(),
        Some("GET /a.gif HTTP/1.0")
    );
    assert!(!m(
        "[ip, user, username, timestamp, request, status_code = 200, bytes]",
        line
    ));
    assert!(m("[..., status_code = 4*, bytes > 2000]", line));
    assert!(m(
        "[..., status_code = 5* || status_code = 404, bytes]",
        line
    ));
    assert!(!m("[..., status_code = 404, bytes > 9999]", line));
    assert!(!m("[a, b]", line));
    assert!(m("[ip, ...]", line));
    let named = p.extracted(line);
    assert!(named.contains(&("$status_code".to_owned(), "404".to_owned())));
}

#[test]
fn a_regular_expression_is_reported_not_guessed() {
    for p in ["%ERR.*%", "{ $.a = %x% }", "[a, b = %x%]"] {
        assert_eq!(parse(p).unwrap_err(), PatternError::Regex, "{p}");
    }
}

#[test]
fn an_unquoted_star_tests_that_a_field_is_there_whatever_its_type() {
    let has = "{ $.f = * }";
    let lacks = "{ $.f != * }";
    for (event, there) in [
        (r#"{"f": "text"}"#, true),
        (r#"{"f": ""}"#, true),
        (r#"{"f": 42}"#, true),
        (r#"{"f": 0.5}"#, true),
        (r#"{"f": true}"#, true),
        (r#"{"f": false}"#, true),
        (r#"{"f": null}"#, true),
        (r#"{"f": {"g": 1}}"#, true),
        (r#"{"g": 1}"#, false),
        (r#"{}"#, false),
    ] {
        assert_eq!(m(has, event), there, "{has} on {event}");
        assert_eq!(m(lacks, event), !there, "{lacks} on {event}");
    }
    // A line that is not JSON is selected by neither.
    assert!(!m(has, "plain text") && !m(lacks, "plain text"));
    // Nested and combined.
    assert!(m("{ $.a.b = * && $.c != * }", r#"{"a": {"b": 7}, "d": 1}"#));
    // A quoted star is a string pattern: it matches any string and nothing else.
    assert!(m(r#"{ $.f = "*" }"#, r#"{"f": "x"}"#));
    assert!(!m(r#"{ $.f = "*" }"#, r#"{"f": 3}"#));
    // Only = and != take a bare star.
    assert_eq!(parse("{ $.f > * }").unwrap_err(), PatternError::Invalid);
}

#[test]
fn globs_match_runs() {
    assert!(glob("*", ""));
    assert!(glob("a*c", "abbbc"));
    assert!(glob("a*c", "ac"));
    assert!(!glob("a*c", "abd"));
    assert!(glob("*b*", "abc"));
    assert!(!glob("abc", "abcd"));
    assert!(glob("abc*", "abcd"));
}
