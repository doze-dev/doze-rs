//! The differential test: every word the Go `internal/display` says over a generated input set (written by
//! `tools/display-oracle` into `tests/fixtures/display.jsonl`), said again by this crate and compared byte for byte.
//! Each line is one case: its kind, its input, and what Go said. A mismatch names the case and both answers.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod console;
mod input;
mod instance;
mod numbers;
mod strings;

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

/// Where `~` stands in every case: the oracle sets `$HOME` to this.
pub(crate) const HOME: &str = "/home/dev";

/// The clock every case is read at: 2026-10-11 14:04:40 UTC.
pub(crate) fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_791_727_480)
}

/// A time written as nanoseconds since the Unix epoch.
pub(crate) fn time(ns: i64) -> SystemTime {
    UNIX_EPOCH + Duration::from_nanos(u64::try_from(ns).expect("a time after 1970"))
}

/// The nanoseconds of a time, as the oracle writes them.
pub(crate) fn nanos(t: SystemTime) -> i64 {
    i64::try_from(t.duration_since(UNIX_EPOCH).expect("after 1970").as_nanos()).expect("fits")
}

/// A float's bits, every NaN written as one, as the oracle writes them.
pub(crate) fn bits(v: f64) -> u64 {
    if v.is_nan() {
        f64::NAN.to_bits()
    } else {
        v.to_bits()
    }
}

/// What this crate says for a case, in the shape the oracle wrote Go's answer in.
fn say(case: &str, input: &Value) -> Value {
    match case {
        "snap" => instance::snapshot(input),
        "clean" | "level" | "fuzzy" | "short_run" | "tilde" | "humanise" | "action_key"
        | "notices" | "parse_float" | "banner" => strings::say(case, input),
        "int" | "duration" | "float" | "span" | "series" => numbers::say(case, input),
        "event" | "history" => instance::journal(case, input),
        "console" => console::say(input),
        other => panic!("the oracle wrote a case this test does not know: {other}"),
    }
}

#[test]
fn says_what_go_says() {
    let golden = include_str!("../fixtures/display.jsonl");
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut wrong = Vec::new();
    for (n, line) in golden.lines().enumerate() {
        let case: Value = serde_json::from_str(line).expect("a JSON line");
        let kind = case["case"].as_str().expect("a case kind");
        *counts.entry(kind.to_owned()).or_default() += 1;
        let got = say(kind, &case["in"]);
        if got != case["out"] {
            wrong.push(format!(
                "line {} ({kind}) in {}\n   go: {}\n rust: {got}",
                n + 1,
                case["in"],
                case["out"]
            ));
        }
    }
    let total: usize = counts.values().sum();
    println!("{total} cases: {counts:?}");
    assert!(total > 10_000, "the golden holds only {total} cases");
    assert!(
        wrong.is_empty(),
        "{} of {total} cases differ from Go; the first:\n{}",
        wrong.len(),
        wrong[..wrong.len().min(20)].join("\n")
    );
}
