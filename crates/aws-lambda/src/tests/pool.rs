//! The pool's limits and its cleanliness: one Runtime API connection per process, a cap on processes in all with the longest idle
//! one making room, no entry kept for a version nothing runs, and nothing a process started outliving it, however it ended.

use std::sync::Arc;
use std::time::Duration;

use doze_runtime::ManualClock;

use serde_json::json;

use super::{World, have};
use crate::{Config, RuntimeConfig};

const HANDLER: &[u8] = b"def handler(e, c):\n    print('line', e.get('n'))\n    return e\n";

#[tokio::test]
async fn the_python_client_keeps_one_connection_to_the_runtime_api() {
    if !have("python3") {
        return;
    }
    let w = World::new();
    w.create(
        "p",
        "python3.12",
        "index.handler",
        &[("index.py", HANDLER, 0o644)],
    )
    .await;
    for n in 0..5 {
        assert_eq!(w.invoke("p", &json!({"n": n})).await["Payload"]["n"], n);
    }
    // Before, each `next` and each `response` was a connection of its own: ten here.
    assert_eq!(w.lambda.inner.rt.connections("us-east-1/p"), [1]);
    w.stop().await;
}

/// Before: idle longest was the millisecond a process was given back, and a warm invocation answered within the millisecond the
/// other process was given back tied with it; the tie went to whichever the pool's map listed first (four failures in 45 runs of
/// the crate). On a clock that does not move every release is the same millisecond, and the order still holds.
#[tokio::test]
async fn a_full_pool_ends_the_process_idle_longest_to_make_room() {
    if !have("python3") {
        return;
    }
    let config = Config {
        quiet: true,
        runtime: RuntimeConfig {
            max_processes: 2,
            ..RuntimeConfig::default()
        },
        ..Config::default()
    };
    let w = World::with_clock(config, Arc::new(ManualClock::at(1_700_000_000_000)));
    for name in ["a", "b", "c"] {
        w.create(
            name,
            "python3.12",
            "index.handler",
            &[("index.py", HANDLER, 0o644)],
        )
        .await;
    }
    w.invoke("a", &json!({})).await;
    w.invoke("b", &json!({})).await;
    w.invoke("a", &json!({})).await;
    // "b" has waited longest: "c" takes its place.
    assert_eq!(w.invoke("c", &json!({"n": 3})).await["Payload"]["n"], 3);
    let rt = &w.lambda.inner.rt;
    for _ in 0..200 {
        if rt.alive("us-east-1/b") == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let alive = |f: &str| rt.alive(&format!("us-east-1/{f}"));
    assert_eq!(
        alive("b"),
        0,
        "b's process is gone: a {} b {} c {}",
        alive("a"),
        alive("b"),
        alive("c")
    );
    assert_eq!((rt.alive("us-east-1/a"), rt.alive("us-east-1/c")), (1, 1));
    w.stop().await;
}

#[tokio::test]
async fn the_entry_of_a_version_nothing_runs_is_let_go() {
    if !have("python3") {
        return;
    }
    let w = World::new();
    w.create(
        "p",
        "python3.12",
        "index.handler",
        &[("index.py", HANDLER, 0o644)],
    )
    .await;
    for n in 0..6 {
        // Each change is a new revision, so a new entry in the pool; the old processes are retired.
        w.ok(
            "UpdateFunctionConfiguration",
            json!({"FunctionName": "p", "Description": format!("rev {n}")}),
        )
        .await;
        w.invoke("p", &json!({})).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        w.lambda.inner.rt.slots() <= 2,
        "{} entries kept",
        w.lambda.inner.rt.slots()
    );
    w.stop().await;
}

/// A `provided` function that starts a child, says its pid, and exits without answering.
const LEAVES_A_CHILD: &[u8] = br#"#!/bin/sh
sleep 300 &
echo $! > "$MARK"
exit 3
"#;

#[cfg(unix)]
#[tokio::test]
async fn a_process_that_exits_by_itself_takes_what_it_started_with_it() {
    if !have("sh") {
        return;
    }
    let w = World::new();
    let mark = w.dir().join("child.pid");
    w.create(
        "x",
        "provided.al2023",
        "bootstrap",
        &[("bootstrap", LEAVES_A_CHILD, 0o755)],
    )
    .await;
    w.ok(
        "UpdateFunctionConfiguration",
        json!({"FunctionName": "x", "Environment": {"Variables": {"MARK": mark.display().to_string()}}}),
    )
    .await;
    let failed = w.invoke("x", &json!({})).await;
    assert_eq!(failed["FunctionError"], "Unhandled");
    let pid = std::fs::read_to_string(&mark).unwrap().trim().to_owned();
    let alive = || {
        std::process::Command::new("kill")
            .args(["-0", &pid])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    w.eventually("the child is gone", || async { !alive() })
        .await;
    w.stop().await;
}
