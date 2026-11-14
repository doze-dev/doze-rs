//! The Rust mariadb module against the Go one's recorded answers (tools/module-oracle). Every case that differs must
//! differ only by the module's listed improvements:
//! - a character set or collation that is not a MariaDB name is refused at its line (Go put it into SQL, and MariaDB
//!   refused it at converge): the corpus's `value-0` and `value-1`;
//! - the readiness ping reads no option file of the machine's (`--no-defaults`);
//! - each grant is an object of its own, so one removed from doze.hcl is revoked (Go tracked databases and users only).
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

/// Whether the case declares a character set or collation that is not a MariaDB name.
fn bad_charset(hcl: &str) -> bool {
    hcl.lines().any(|l| {
        let l = l.trim();
        (l.starts_with("character_set") || l.starts_with("collation"))
            && l.split('"')
                .nth(1)
                .is_some_and(|v| !v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
    })
}

/// The grants a case declares, as the objects Rust lists for them.
fn grants(hcl: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut blocks = hcl.split("grant {").skip(1);
    for b in blocks.by_ref() {
        let body = b.split('}').next().unwrap_or_default();
        let field = |k: &str| {
            body.lines()
                .find(|l| l.trim().starts_with(k))
                .and_then(|l| l.split('"').nth(1))
                .map(str::to_owned)
        };
        let db = field("database").map_or("*".into(), |d| format!("`{d}`"));
        let name = format!(
            "{}@{} on {db}.*",
            field("user").unwrap(),
            field("host").unwrap_or_else(|| "%".into())
        );
        out.push(json!({ "kind": "grant", "name": name }));
    }
    out
}

#[tokio::test(flavor = "current_thread")]
async fn answers_as_the_go_module_did_except_for_the_listed_improvements() {
    let m = Harness {
        program: env!("CARGO_BIN_EXE_doze-module-mariadb").into(),
        engine: "mariadb".into(),
        bin_dir: "/tc/bin".into(),
        full: "9.9.9".into(),
    }
    .module()
    .await
    .unwrap();
    let diffs = oracle::compare(&m, "mariadb", CORPUS, GOLDEN, &[]).await;
    assert!(!diffs.is_empty(), "the improvements must show");
    for d in &diffs {
        let hcl = case(&d.id);
        if bad_charset(&hcl) {
            assert_eq!(d.go["refused"], false, "{}", d.id);
            assert_eq!(d.rust["refused"], true, "{} must be refused:\n{d:#?}", d.id);
            continue;
        }
        assert_eq!(
            d.go["refused"], d.rust["refused"],
            "{} is refused by one side only:\n{d:#?}",
            d.id
        );
        let mut want = d.go.clone();
        // The readiness ping reads no option file of the machine's.
        let ready = want["plan"][0]["ready"]
            .as_str()
            .unwrap()
            .replace("mariadb-admin ", "mariadb-admin --no-defaults ");
        want["plan"][0]["ready"] = json!(ready);
        want["objects"].as_array_mut().unwrap().extend(grants(&hcl));
        assert_eq!(
            want, d.rust,
            "{} differs beyond the listed improvements",
            d.id
        );
    }
}
