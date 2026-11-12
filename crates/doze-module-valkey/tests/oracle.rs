//! The Rust valkey module against the Go one's recorded answers (tools/module-oracle): the same refusals, URLs and
//! plans, except where the password goes. Those cases are not waved through: each must differ in exactly that way.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_module_test::{Harness, oracle};
use serde_json::json;

const CORPUS: &str = include_str!("../../../tools/module-oracle/corpus.jsonl");
const GOLDEN: &str = include_str!("../../../tools/module-oracle/golden.jsonl");

#[tokio::test(flavor = "current_thread")]
async fn answers_as_the_go_module_did_except_where_the_password_goes() {
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
    let mut moved = Vec::new();
    for d in &diffs {
        // Go put `--requirepass P` on the command line; Rust reads it from a private file given first (bug 7).
        let mut go = d.go.clone();
        let name = go["plan"][0]["name"].as_str().unwrap().to_string();
        let args = go["plan"][0]["args"].as_array_mut().unwrap();
        let Some(at) = args.iter().position(|a| a == "--requirepass") else {
            panic!("{} differs for another reason:\n{d:#?}", d.id)
        };
        args.drain(at..at + 2);
        args.insert(0, json!(format!("/tmp/dzo/d/{name}/doze-auth.conf")));
        assert_eq!(go, d.rust, "{} differs beyond the password", d.id);
        moved.push(d.id.as_str());
    }
    assert_eq!(
        moved,
        [
            "valkey/000-example",
            "valkey/006-arg-password",
            "valkey/012-arg-password-alt",
            "valkey/015-all-args",
            "valkey/017-pair-appendonly-password"
        ]
    );
}
