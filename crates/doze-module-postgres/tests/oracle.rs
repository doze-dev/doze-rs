//! The Rust postgres module against the Go one's recorded answers (tools/module-oracle). Every case that differs must
//! differ only by the module's listed improvements:
//! - each grant is an object of its own, so one removed from doze.hcl is revoked (Go did not track grants);
//! - two `schema` blocks with one name are refused at the second (Go made the schema once and listed it twice);
//! - an extension `source` that is not a file is refused at its line unless the extension is `optional` (Go failed at
//!   converge).
//!
//! Object hashes are not compared: Go's are SHA-256 of Go's JSON, the protocol's are of CBOR (protocol.md row 15);
//! kinds, names and order are.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_module_test::{Harness, oracle};
use serde_json::{Value, json};

const CORPUS: &str = include_str!("../../../tools/module-oracle/corpus.jsonl");
const GOLDEN: &str = include_str!("../../../tools/module-oracle/golden.jsonl");

fn case(id: &str) -> String {
    CORPUS
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|c| c["id"] == id)
        .unwrap()["hcl"]
        .as_str()
        .unwrap()
        .to_string()
}

/// The quoted value of `key = "…"` in a block body.
fn field(body: &str, key: &str) -> Option<String> {
    body.lines()
        .find(|l| l.trim().starts_with(&format!("{key} ")))
        .and_then(|l| l.split('"').nth(1))
        .map(str::to_owned)
}

/// The grants a case declares, as the objects Rust lists for them.
fn grants(hcl: &str) -> Vec<Value> {
    hcl.split("grant {")
        .skip(1)
        .map(|b| {
            let body = b.split('}').next().unwrap_or_default();
            let role = field(body, "role").unwrap();
            let on = match (
                field(body, "database"),
                field(body, "schema"),
                field(body, "objects"),
            ) {
                (Some(d), None, None) => format!("database \"{d}\""),
                (None, Some(s), None) => format!("schema \"{s}\""),
                (None, Some(s), Some(o)) => format!("{o} in schema \"{s}\""),
                other => panic!("a grant the corpus should not hold: {other:?}"),
            };
            json!({ "kind": "grant", "name": format!("\"{role}\" on {on}") })
        })
        .collect()
}

/// Whether a non-optional extension block of the case names a `source` (none of the corpus's exist on disk).
fn missing_source(hcl: &str) -> bool {
    hcl.split("extension \"").skip(1).any(|b| {
        let body = b.split('}').next().unwrap_or_default();
        field(body, "source").is_some() && !body.contains("optional = true")
    })
}

#[tokio::test(flavor = "current_thread")]
async fn answers_as_the_go_module_did_except_for_the_listed_improvements() {
    let m = Harness {
        program: env!("CARGO_BIN_EXE_doze-module-postgres").into(),
        engine: "postgres".into(),
        bin_dir: "/tc/bin".into(),
        full: "9.9.9".into(),
    }
    .module()
    .await
    .unwrap();
    let diffs = oracle::compare(&m, "postgres", CORPUS, GOLDEN, &[]).await;
    assert!(!diffs.is_empty(), "the improvements must show");
    for d in &diffs {
        let hcl = case(&d.id);
        if d.id.ends_with("-schema-duplicate") || missing_source(&hcl) {
            assert_eq!(d.go["refused"], false, "{}", d.id);
            assert_eq!(d.rust["refused"], true, "{} must be refused:\n{d:#?}", d.id);
            continue;
        }
        let mut want = d.go.clone();
        want["objects"].as_array_mut().unwrap().extend(grants(&hcl));
        assert_eq!(
            want, d.rust,
            "{} differs beyond the listed improvements",
            d.id
        );
    }
}
