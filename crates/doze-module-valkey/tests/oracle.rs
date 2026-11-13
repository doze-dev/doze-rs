//! The Rust valkey module against the Go one's recorded answers (tools/module-oracle). Everything must match except
//! the module's two deliberate improvements, and those are not waved through: each differing case must differ in
//! exactly those ways and no other.
//! - Bug 7: Go put `--requirepass P` on the command line; Rust gives a private config file as the first argument.
//! - Sleep keeps data: with no `save` in the block, Rust adds `--shutdown-on-sigterm save` after `--appendonly`.
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

/// Go's answer for `id`, with the two improvements applied: what Rust must answer.
fn improved(id: &str, go: &Value) -> Value {
    let mut go = go.clone();
    let case: Value = CORPUS
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|c| c["id"] == id)
        .unwrap();
    let sets_save = case["hcl"].as_str().unwrap().contains("save");
    let name = go["plan"][0]["name"].as_str().unwrap().to_string();
    let args = go["plan"][0]["args"].as_array_mut().unwrap();
    if let Some(at) = args.iter().position(|a| a == "--requirepass") {
        args.drain(at..at + 2);
        args.insert(0, json!(format!("/tmp/dzo/d/{name}/doze-auth.conf")));
    }
    if !sets_save {
        let at = args.iter().position(|a| a == "--appendonly").unwrap() + 2;
        args.insert(at, json!("save"));
        args.insert(at, json!("--shutdown-on-sigterm"));
    }
    go
}

#[tokio::test(flavor = "current_thread")]
async fn answers_as_the_go_module_did_except_for_the_two_improvements() {
    let m = Harness {
        program: env!("CARGO_BIN_EXE_doze-module-valkey").into(),
        engine: "valkey".into(),
        bin_dir: "/tc/bin".into(),
        full: "9.9.9".into(),
    }
    .module()
    .await
    .unwrap();
    let diffs = oracle::compare(&m, "valkey", CORPUS, GOLDEN, &[]).await;
    assert!(!diffs.is_empty(), "the improvements must show");
    let mut refused_early = Vec::new();
    for d in &diffs {
        assert_eq!(
            d.go["refused"], false,
            "{} is refused by Go only:\n{d:#?}",
            d.id
        );
        if d.rust["refused"] == true {
            refused_early.push(d.id.as_str());
            continue;
        }
        assert_eq!(
            improved(&d.id, &d.go),
            d.rust,
            "{} differs beyond the two improvements",
            d.id
        );
    }
    // The third improvement: values valkey itself refuses at boot (a policy of "256MB", a schedule of "value-4") are
    // refused at decode instead. tests/acceptance.rs proves valkey refuses each of them.
    assert_eq!(
        refused_early,
        [
            "valkey/004-arg-maxmemory_policy",
            "valkey/007-arg-save",
            "valkey/010-arg-maxmemory_policy-alt",
            "valkey/013-arg-save-alt",
            "valkey/015-all-args",
            "valkey/016-pair-maxmemory-maxmemory_policy",
            "valkey/018-pair-save-settings",
        ]
    );
}
