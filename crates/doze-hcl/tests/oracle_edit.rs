//! The editor against Go's `internal/config` Edit (`oracle-dump editcases`): `doze add`'s
//! AppendBlock and `doze init`'s SetStack on every case, byte for byte, plus the declared names.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use doze_hcl::edit::Edit;
use serde_json::{Value as Json, json};

/// What the oracle appends (tools/oracle-dump/syntax.go `appendedBlock`).
const APPENDED: &str = "app \"extra\" {\n  run = \"go run ./cmd/extra\"\n  port=9090\n}\n";

fn ours(src: &str) -> Json {
    let Ok(e) = Edit::open(src) else {
        return json!({"open": false});
    };
    let names: Vec<String> = e
        .blocks()
        .into_iter()
        .filter_map(|(_, l)| l.last().cloned())
        .collect();
    let mut a = e.clone();
    let append = a
        .append_block(APPENDED)
        .map(|()| String::from_utf8(a.bytes()).unwrap());
    let mut s = e.clone();
    s.set_top_attribute("stack", "shop");
    json!({
        "open": true,
        "names": if names.is_empty() { Json::Null } else { json!(names) },
        "has_stack": e.has_attribute("stack"),
        "append": append.ok(),
        "stack": String::from_utf8(s.bytes()).unwrap(),
    })
}

#[test]
fn edits_match_go() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/edit");
    let cases: Vec<Json> =
        serde_json::from_str(&std::fs::read_to_string(format!("{dir}/cases.json")).unwrap())
            .unwrap();
    let go: Vec<Json> = std::fs::read_to_string(format!("{dir}/go.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(cases.len(), go.len());
    let mut diffs = Vec::new();
    for (c, g) in cases.iter().zip(&go) {
        let mut want = g.clone();
        want.as_object_mut().unwrap().remove("id");
        if want["open"] == json!(true) && want.get("append").is_none() {
            want["append"] = Json::Null;
        }
        let got = ours(c["src"].as_str().unwrap());
        if got != want {
            diffs.push(format!(
                "{}: {:?}\n   go:   {want}\n   rust: {got}",
                c["id"], c["src"]
            ));
        }
    }
    eprintln!(
        "edit: {} of {} identical",
        cases.len() - diffs.len(),
        cases.len()
    );
    assert!(
        diffs.is_empty(),
        "{} differ:\n{}",
        diffs.len(),
        diffs.iter().take(8).cloned().collect::<Vec<_>>().join("\n")
    );
}
