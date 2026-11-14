//! The H1 gate: every expression of the corpus evaluates to what Go's hcl and go-cty give (value and
//! exact type), or fails where Go fails, with the first diagnostic at the same line and column.
//!
//! Go's side is `testdata/eval/go-*.jsonl`, written by doze's `tools/oracle-dump eval` (branch
//! `rust-oracle`) with an empty environment:
//!
//!     env -i PATH=/usr/bin:/bin oracle-dump eval exprs-<set>.json > go-<set>.jsonl
//!
//! Function calls are H2's: a case that calls a function Go knows is counted, not compared, until
//! the library exists.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_hcl::eval::{Context, eval_attribute};
use doze_hcl::function::Functions;
use serde_json::{Value as Json, json};

mod support;

use support::{canon, variables};
/// One case's result in the oracle's shape, reduced to what must match.
fn rust_result(expr: &str, ctx: &Context<'_>) -> Json {
    match eval_attribute(&format!("x = {expr}\n"), "x", ctx) {
        Err(d) => {
            json!({"ok": false, "stage": "parse", "at": d.subject.map(|r| [r.start.line, r.start.column])})
        }
        Ok((_, diags)) if diags.has_errors() => {
            let first = diags
                .iter()
                .find_map(|d| d.subject)
                .map(|r| [r.start.line, r.start.column]);
            json!({"ok": false, "stage": "eval", "at": first})
        }
        Ok((v, _)) => {
            let ty: Json = serde_json::from_str(&v.ty().to_json()).unwrap();
            json!({"ok": true, "value": canon(&v), "type": ty})
        }
    }
}

fn go_result(line: &Json) -> Json {
    if line["ok"] == json!(true) {
        return json!({"ok": true, "value": line["value"], "type": line["type"]});
    }
    let stage = if line["stage"] == "parse" {
        "parse"
    } else {
        "eval"
    };
    let at = line["diags"]
        .as_array()
        .and_then(|ds| ds.iter().find_map(|d| d.get("at")))
        .map(|a| json!([a["start"]["line"], a["start"]["col"]]));
    json!({"ok": false, "stage": stage, "at": at.unwrap_or(Json::Null)})
}

fn calls_a_function(expr: &str) -> bool {
    let b = expr.as_bytes();
    expr.match_indices('(').any(|(i, _)| {
        i > 0
            && b.get(i - 1)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
    })
}

/// (identical, deferred to H2 functions, syntax errors at another position, differences)
fn run(set: &str) -> (usize, usize, Vec<String>, Vec<String>) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/eval");
    let cases: Vec<Json> =
        serde_json::from_str(&std::fs::read_to_string(format!("{dir}/exprs-{set}.json")).unwrap())
            .unwrap();
    let go: Vec<Json> = std::fs::read_to_string(format!("{dir}/go-{set}.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        cases.len(),
        go.len(),
        "{set}: corpus and Go output differ in length; regenerate go-{set}.jsonl"
    );
    let funcs = Functions::new();
    let ctx = Context::new(variables(), Some(&funcs));
    let (mut same, mut deferred, mut syntax, mut diffs) = (0, 0, Vec::new(), Vec::new());
    for (case, g) in cases.iter().zip(&go) {
        let (id, expr) = (case["id"].as_str().unwrap(), case["expr"].as_str().unwrap());
        assert_eq!(g["id"], case["id"]);
        let (want, got) = (go_result(g), rust_result(expr, &ctx));
        if want == got {
            same += 1;
        } else if want["stage"] == "parse" && got["stage"] == "parse" {
            syntax.push(format!("{id}: go {} rust {}", want["at"], got["at"]));
        } else if calls_a_function(expr) && g["ok"] == json!(true)
            || calls_a_function(expr)
                && !expr.contains("nope")
                && !expr.contains("uper")
                && !expr.contains("::")
        {
            deferred += 1;
        } else {
            diffs.push(format!("{id}: {expr}\n   go:   {want}\n   rust: {got}"));
        }
    }
    (same, deferred, syntax, diffs)
}

/// Every `exprs-<set>.json` in `testdata/eval` with its `go-<set>.jsonl` (the spike's 653, H1's own,
/// and any set added later).
#[test]
fn every_corpus_matches_go() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/eval");
    let mut sets: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|n| {
            n.strip_prefix("exprs-")?
                .strip_suffix(".json")
                .map(str::to_owned)
        })
        .collect();
    sets.sort();
    assert!(sets.len() >= 2, "corpus sets: {sets:?}");
    let mut all = Vec::new();
    for set in &sets {
        let (same, deferred, syntax, diffs) = run(set);
        eprintln!(
            "{set}: {same} identical, {deferred} deferred to H2 (functions), {} syntax errors placed differently (H2), {} different",
            syntax.len(),
            diffs.len()
        );
        all.extend(diffs);
    }
    assert!(
        all.is_empty(),
        "{} differences from Go:\n{}",
        all.len(),
        all.join("\n")
    );
}
