//! The Rust temporal module against the Go one's recorded answers (tools/module-oracle). The oracle grants temporal's
//! TCP listeners the conventional ports Go bound itself, so the plans compare argument for argument; every case that
//! differs must differ only by the module's listed improvements:
//! - ready means `temporal operator cluster health` says SERVING, not that the port opened;
//! - headless has no Web UI listener, so no `--ui-port` (Go passed one for a UI it then disabled);
//! - declared namespaces are passed to `start-dev --namespace`, so they exist the moment it is ready.
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

/// The namespace labels declared in a case's HCL, in order.
fn namespaces(hcl: &str) -> Vec<String> {
    hcl.lines()
        .filter_map(|l| l.trim().strip_prefix("namespace \""))
        .filter_map(|l| l.split('"').next().map(String::from))
        .collect()
}

/// Go's answer for `id` with the improvements applied: what Rust must answer.
fn improved(id: &str, go: &Value) -> Value {
    let hcl = case(id);
    let mut go = go.clone();
    let p = &mut go["plan"][0];
    let port = p["args"]
        .as_array()
        .unwrap()
        .iter()
        .position(|a| a == "--port")
        .unwrap()
        + 1;
    let main = format!("127.0.0.1:{}", p["args"][port].as_str().unwrap());
    p["ready"] = json!(format!(
        "exec /tc/bin/temporal operator cluster health --address {main}"
    ));
    let args = p["args"].as_array_mut().unwrap();
    if hcl.contains("headless = true") {
        let at = args.iter().position(|a| a == "--ui-port").unwrap();
        args.drain(at..at + 2);
    }
    for ns in namespaces(&hcl) {
        args.push(json!("--namespace"));
        args.push(json!(ns));
    }
    go
}

#[tokio::test(flavor = "current_thread")]
async fn answers_as_the_go_module_did_except_for_the_listed_improvements() {
    let m = Harness {
        program: env!("CARGO_BIN_EXE_doze-module-temporal").into(),
        engine: "temporal".into(),
        bin_dir: "/tc/bin".into(),
        full: "9.9.9".into(),
    }
    .module()
    .await
    .unwrap();
    let diffs = oracle::compare(&m, "temporal", CORPUS, GOLDEN, &[]).await;
    assert!(!diffs.is_empty(), "the improvements must show");
    for d in &diffs {
        assert_eq!(
            d.go["refused"], d.rust["refused"],
            "{} is refused by one side only:\n{d:#?}",
            d.id
        );
        if d.go["refused"] == true {
            continue;
        }
        assert_eq!(
            improved(&d.id, &d.go),
            d.rust,
            "{} differs beyond the listed improvements",
            d.id
        );
    }
}
