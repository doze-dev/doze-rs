//! `FilterLogEvents` reads a bounded number of rows a call and goes on by its token; the streams are merged in order; the filters a
//! `PutLogEvents` runs are parsed once.

use serde_json::{Value, json};

use super::World;
use crate::merge::SCAN_MAX;

fn filter(w: &mut World, request: Value) -> (Vec<String>, Option<String>) {
    let out = w.ok("FilterLogEvents", &request);
    let events = out["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            format!(
                "{}:{}",
                e["logStreamName"].as_str().unwrap(),
                e["message"].as_str().unwrap()
            )
        })
        .collect();
    (events, out["nextToken"].as_str().map(str::to_owned))
}

/// Before: a filter that matches little read every event of the group in one call. After: at most `SCAN_MAX` rows, then a token.
#[test]
fn a_filter_reads_a_bounded_number_of_rows_a_call_and_goes_on_by_its_token() {
    let mut w = World::new();
    w.group("/g");
    w.stream("/g", "s");
    let noise = vec!["noise"; 10_000];
    let start = super::NOW - 86_400_000;
    for batch in 0..3 {
        w.put("/g", "s", start + batch * 10_000, &noise);
    }
    w.put("/g", "s", start + 30_000, &["needle"]);
    let mut request = json!({"logGroupName": "/g", "filterPattern": "needle"});
    let (first, mut token) = filter(&mut w, request.clone());
    assert!(first.is_empty(), "{first:?}");
    assert!(
        token.is_some(),
        "the first call stops after {SCAN_MAX} rows"
    );
    let mut found = Vec::new();
    let mut calls = 1;
    while let Some(t) = token {
        request["nextToken"] = json!(t);
        let (events, next) = filter(&mut w, request.clone());
        found.extend(events);
        token = next;
        calls += 1;
        assert!(calls < 10, "the token goes on");
    }
    assert_eq!(found, ["s:needle"]);
    assert_eq!(calls, 4, "30,001 rows at {SCAN_MAX} a call");
}

#[test]
fn streams_are_merged_in_order_across_pages() {
    let mut w = World::new();
    w.group("/g");
    for s in ["a", "b"] {
        w.stream("/g", s);
    }
    let start = super::NOW - 1_000;
    // a at 0, 2, 4; b at 1, 2 (same time as a's second: the stream name orders them), 5.
    for (s, at, m) in [
        ("a", 0, "a0"),
        ("a", 2, "a2"),
        ("a", 4, "a4"),
        ("b", 1, "b1"),
        ("b", 2, "b2"),
        ("b", 5, "b5"),
    ] {
        w.put("/g", s, start + at, &[m]);
    }
    let mut request = json!({"logGroupName": "/g", "limit": 4});
    let (first, token) = filter(&mut w, request.clone());
    assert_eq!(first, ["a:a0", "b:b1", "a:a2", "b:b2"]);
    request["nextToken"] = json!(token.unwrap());
    let (rest, token) = filter(&mut w, request);
    assert_eq!(rest, ["a:a4", "b:b5"]);
    assert_eq!(token, None);
}

/// Before: each metric and subscription filter's pattern was parsed on every `PutLogEvents`.
#[test]
fn a_filter_pattern_is_parsed_once() {
    let mut w = World::new();
    w.group("/g");
    w.stream("/g", "s");
    let metric = |pattern: &str| {
        json!({"logGroupName": "/g", "filterName": "m", "filterPattern": pattern,
            "metricTransformations": [{"metricName": "m", "metricNamespace": "n", "metricValue": "1"}]})
    };
    w.ok("PutMetricFilter", &metric("%ERR[0-9]+%"));
    w.put("/g", "s", super::NOW - 10, &["ERR1"]);
    w.put("/g", "s", super::NOW - 5, &["ERR2"]);
    assert_eq!(w.core.filter_patterns.len(), 1);
    // A changed filter is a new pattern; the old one is never used again.
    w.ok("PutMetricFilter", &metric("%WARN%"));
    w.put("/g", "s", super::NOW - 1, &["WARN"]);
    assert_eq!(w.core.filter_patterns.len(), 2);
}
