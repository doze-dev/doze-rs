//! The H1 syntax gate: the 190 real sources of the HCL spike corpus (docs fences, `.hcl` files, Go
//! test strings; `testdata/sources/index.json` says where each came from) are accepted or refused
//! exactly as Go's `hclsyntax.ParseConfig` does (`testdata/go-parse.jsonl`, from
//! `oracle-dump parse testdata/sources`). Where both refuse, the error position is H2's to match.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_hcl::syntax::{Source, parse_body};
use serde_json::Value as Json;

#[test]
fn sources_accept_and_refuse_as_go_does() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata");
    let go = std::fs::read_to_string(format!("{dir}/go-parse.jsonl")).unwrap();
    let (mut same, mut both_refuse, mut diffs) = (0, 0, Vec::new());
    for line in go.lines() {
        let g: Json = serde_json::from_str(line).unwrap();
        let id = g["id"].as_str().unwrap();
        let text = std::fs::read_to_string(format!("{dir}/sources/{id}.hcl")).unwrap();
        let go_ok = g["r"]["ok"] == Json::Bool(true);
        let rust = parse_body(&Source::new(&text));
        match (go_ok, &rust) {
            (true, Ok(_)) => same += 1,
            (false, Err(_)) => {
                same += 1;
                both_refuse += 1;
            }
            (true, Err(d)) => diffs.push(format!("{id}: Go accepts, doze-hcl refuses: {d}")),
            (false, Ok(_)) => diffs.push(format!(
                "{id}: Go refuses ({}), doze-hcl accepts",
                g["r"]["diags"][0]["summary"]
            )),
        }
    }
    eprintln!(
        "sources: {same} agree ({both_refuse} refused by both), {} differ",
        diffs.len()
    );
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
    assert_eq!(same, 190);
}
