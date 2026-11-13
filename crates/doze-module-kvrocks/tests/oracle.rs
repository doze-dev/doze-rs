//! The Rust kvrocks module against the Go one's recorded answers (tools/module-oracle): the same refusals, URLs and
//! plans on every case. (The config file it writes is covered by the unit tests in src/conf.rs, ported from Go's.)
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_module_test::{Harness, oracle};

const CORPUS: &str = include_str!("../../../tools/module-oracle/corpus.jsonl");
const GOLDEN: &str = include_str!("../../../tools/module-oracle/golden.jsonl");

#[tokio::test(flavor = "current_thread")]
async fn answers_as_the_go_module_did() {
    let m = Harness {
        program: env!("CARGO_BIN_EXE_doze-module-kvrocks").into(),
        engine: "kvrocks".into(),
        bin_dir: "/tc/bin".into(),
        full: "9.9.9".into(),
    }
    .module()
    .await
    .unwrap();
    let diffs = oracle::compare(&m, "kvrocks", CORPUS, GOLDEN, &[]).await;
    assert!(diffs.is_empty(), "{diffs:#?}");
}
