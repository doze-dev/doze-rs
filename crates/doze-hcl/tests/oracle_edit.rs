//! The editor against Go's `internal/config` Edit (`oracle-dump editcases`, and the H3 audit's Go
//! replica for cases with their own block or stack name): `doze add`'s AppendBlock and `doze
//! init`'s SetStack, byte for byte, plus the declared names.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use doze_hcl::edit::Edit;
use serde_json::{Value as Json, json};

/// What the oracle appends unless a case says otherwise (tools/oracle-dump/syntax.go).
const APPENDED: &str = "app \"extra\" {\n  run = \"go run ./cmd/extra\"\n  port=9090\n}\n";

/// Reviewed differences: Go escapes unassigned code points and the format character U+1D173 in a
/// stack name (`unicode.IsPrint`); without general-category tables doze-hcl writes them as they are.
const KNOWN: [&str; 6] = ["e.225", "e.226", "e.269", "e.270", "e.313", "e.314"];

fn ours(c: &Json) -> Json {
    let Ok(e) = Edit::open(c["src"].as_str().unwrap()) else {
        return json!({"open": false});
    };
    let names: Vec<String> = e
        .blocks()
        .into_iter()
        .filter_map(|(_, l)| l.last().cloned())
        .collect();
    let mut a = e.clone();
    let block = c.get("block").and_then(Json::as_str).unwrap_or(APPENDED);
    let append = a
        .append_block(block)
        .map(|()| String::from_utf8(a.bytes()).unwrap());
    let mut s = e.clone();
    s.set_top_attribute(
        "stack",
        c.get("stack").and_then(Json::as_str).unwrap_or("shop"),
    );
    json!({
        "open": true,
        "names": if names.is_empty() { Json::Null } else { json!(names) },
        "has_stack": e.has_attribute("stack"),
        "append": append.ok(),
        "stack": String::from_utf8(s.bytes()).unwrap(),
    })
}

fn run(set: &str, go_set: &str) -> (usize, Vec<String>) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/edit");
    let cases: Vec<Json> =
        serde_json::from_str(&std::fs::read_to_string(format!("{dir}/{set}.json")).unwrap())
            .unwrap();
    let go: Vec<Json> = std::fs::read_to_string(format!("{dir}/{go_set}.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(cases.len(), go.len(), "{set}");
    let mut diffs = Vec::new();
    for (c, g) in cases.iter().zip(&go) {
        let mut want = g.clone();
        let obj = want.as_object_mut().unwrap();
        obj.remove("id");
        if obj.remove("append_err").is_some()
            || obj.get("open") == Some(&json!(true)) && !obj.contains_key("append")
        {
            obj.insert("append".into(), Json::Null);
        }
        if obj.get("names") == Some(&json!([])) {
            obj.insert("names".into(), Json::Null);
        }
        let got = ours(c);
        let known = KNOWN.contains(&c["id"].as_str().unwrap());
        match (got == want, known) {
            (true, true) => diffs.push(format!("{}: known difference now matches Go", c["id"])),
            (false, false) => diffs.push(format!(
                "{}: {:?}\n   go:   {want}\n   rust: {got}",
                c["id"], c["src"]
            )),
            _ => {}
        }
    }
    (cases.len(), diffs)
}

#[test]
fn edits_match_go() {
    let mut all = Vec::new();
    for (set, go) in [
        ("cases", "go"),
        ("cases-audit", "go-audit"),
        ("cases-labels", "go-labels"),
    ] {
        let (n, diffs) = run(set, go);
        eprintln!("edit {set}: {} of {n} identical", n - diffs.len());
        all.extend(diffs);
    }
    assert!(
        all.is_empty(),
        "{} differ:\n{}",
        all.len(),
        all.iter().take(8).cloned().collect::<Vec<_>>().join("\n")
    );
}
