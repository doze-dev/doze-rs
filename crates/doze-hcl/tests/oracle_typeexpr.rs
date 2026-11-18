//! Type expressions against Go's `typeexpr.TypeConstraint` and `typeexpr.Type` (`oracle-dump
//! typeexpr`): the type, or every diagnostic's range and detail.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_hcl::syntax::{Source, parse_body};
use doze_hcl::typeexpr::{exact_type, type_constraint};
use serde_json::{Value as Json, json};

fn ours(r: (doze_hcl::Type, doze_hcl::diag::Diagnostics)) -> Json {
    let (t, ds) = r;
    if ds.has_errors() {
        let d: Vec<Json> = ds
            .iter()
            .map(|d| {
                let r = d.subject.unwrap_or_default();
                json!([
                    r.start.line,
                    r.start.column,
                    r.end.line,
                    r.end.column,
                    d.detail
                ])
            })
            .collect();
        return json!({"ok": false, "d": d});
    }
    json!({"ok": true, "type": serde_json::from_str::<Json>(&t.to_json()).unwrap()})
}

fn theirs(r: &Json) -> Json {
    if r["ok"] == json!(true) {
        return json!({"ok": true, "type": r["type"]});
    }
    let d: Vec<Json> = r["diags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            let (s, e) = (&d["at"]["start"], &d["at"]["end"]);
            json!([s["line"], s["col"], e["line"], e["col"], d["detail"]])
        })
        .collect();
    json!({"ok": false, "d": d})
}

#[test]
fn type_expressions_match_go() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/typeexpr");
    let cases: Vec<Json> =
        serde_json::from_str(&std::fs::read_to_string(format!("{dir}/exprs.json")).unwrap())
            .unwrap();
    let go: Vec<Json> = std::fs::read_to_string(format!("{dir}/go.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(cases.len(), go.len());
    let mut diffs = Vec::new();
    for (c, g) in cases.iter().zip(&go) {
        let text = format!("x = {}\n", c["expr"].as_str().unwrap());
        let src = Source::new(&text);
        let Ok(body) = parse_body(&src) else {
            if g["stage"] != "parse" {
                diffs.push(format!("{}: refused to parse, Go parsed it", c["id"]));
            }
            continue;
        };
        let expr = &body.get_attribute("x").unwrap().value;
        for (mode, got) in [
            ("constraint", ours(type_constraint(&src, expr))),
            ("exact", ours(exact_type(&src, expr))),
        ] {
            let want = theirs(&g[mode]);
            if got != want {
                diffs.push(format!(
                    "{} {mode}: {}\n   go:   {want}\n   rust: {got}",
                    c["id"], c["expr"]
                ));
            }
        }
    }
    eprintln!("typeexpr: {} cases, {} differ", cases.len(), diffs.len());
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
}
