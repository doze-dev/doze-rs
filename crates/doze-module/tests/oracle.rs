//! The Go-versus-Rust comparison (doze-module-test::oracle) proved on the echo module against hand-written "Go" answers:
//! it reports nothing when they agree, reports exactly the case that differs, and a declared deviation silences it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_module_test::{Harness, oracle};
use serde_json::json;
use std::path::PathBuf;

fn harness() -> Harness {
    let engine = PathBuf::from(env!("CARGO_BIN_EXE_doze-module-echo-engine"));
    Harness {
        program: env!("CARGO_BIN_EXE_doze-module-echo").into(),
        engine: "echo".into(),
        bin_dir: engine.parent().unwrap().into(),
        full: "2.0.0".into(),
    }
}

fn corpus() -> String {
    let cases = [
        json!({ "id": "echo/000-plain", "engine": "echo", "version": "2", "hcl": "echo \"t\" {\n  version = \"2\"\n  channel \"a\" {}\n}\n" }),
        json!({ "id": "echo/001-unknown", "engine": "echo", "version": "2", "hcl": "echo \"t\" {\n  version = \"2\"\n  bogus = 1\n}\n" }),
        json!({ "id": "echo/002-buffer", "engine": "echo", "version": "2", "hcl": "echo \"t\" {\n  version = \"2\"\n  buffer = 64\n}\n" }),
    ];
    cases
        .iter()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// What a Go echo module would have answered, in the oracle's shape.
fn golden(buffer_for_002: &str) -> String {
    let sock = "/tmp/dzo/s/t/echo.sock";
    let program = format!(
        "/tc/bin/doze-module-echo-engine{}",
        std::env::consts::EXE_SUFFIX
    );
    let plan = |buffer: &str| {
        json!([{ "name": "echo", "program": program, "args": ["--listen", format!("unix:{sock}"), "--buffer", buffer],
                 "env_added": {}, "after": null, "ready": format!("socket {sock}"), "hooks": null }])
    };
    let ok = |id: &str, buffer: &str, objects: serde_json::Value| {
        json!({ "id": id, "engine": "echo", "backend": sock, "env_var": "ECHO_URL", "url": "echo://127.0.0.1:7777/t",
                "objects": objects, "plan": plan(buffer) })
    };
    let lines = [
        ok(
            "echo/000-plain",
            "10",
            json!([{ "kind": "channel", "name": "a", "hash": "go-hash" }]),
        ),
        json!({ "id": "echo/001-unknown", "engine": "echo", "decode_error": "Unsupported argument; An argument named \"bogus\" is not expected here." }),
        ok("echo/002-buffer", buffer_for_002, json!([])),
    ];
    lines
        .iter()
        .map(|l| l.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test(flavor = "current_thread")]
async fn agreeing_answers_report_nothing_and_a_difference_names_its_case() {
    let m = harness().module().await.unwrap();
    let diffs = oracle::compare(&m, "echo", &corpus(), &golden("64"), &[]).await;
    assert!(diffs.is_empty(), "{diffs:#?}");
    let diffs = oracle::compare(&m, "echo", &corpus(), &golden("32"), &[]).await;
    assert_eq!(
        diffs.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(),
        ["echo/002-buffer"]
    );
    let diffs = oracle::compare(&m, "echo", &corpus(), &golden("32"), &["echo/002-buffer"]).await;
    assert!(
        diffs.is_empty(),
        "a declared deviation must not be reported"
    );
}
