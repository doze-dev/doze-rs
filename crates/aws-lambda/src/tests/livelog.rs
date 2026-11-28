//! What a function prints reaches its log group while it runs, not only when it ends, and the whole log is still `START`, the lines,
//! `END` and `REPORT`, each once.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::json;

use super::sources::logged;
use super::{World, have};

/// The handler sleeps 3 s between its lines: the first must reach Logs (a 200 ms tick, then CreateLogGroup, CreateLogStream and
/// PutLogEvents, each a durable commit) and be read back by FilterLogEvents within it. With 1.5 s it failed 2 times in about 130 runs
/// in the Linux VM (2026-11-28), with no cause found; if it fails again, the message gives every poll's timing.
#[tokio::test]
async fn a_line_is_in_the_log_before_the_invocation_ends() {
    if !have("python3") {
        return;
    }
    let w = World::new();
    w.create(
        "slow",
        "python3.12",
        "index.handler",
        &[(
            "index.py",
            b"import time\ndef handler(e, c):\n    print('first', flush=True)\n    time.sleep(3)\n    print('second', flush=True)\n    return 1\n",
            0o644,
        )],
    )
    .await;
    let done = AtomicBool::new(false);
    let began = std::time::Instant::now();
    let invoke = async {
        w.invoke("slow", &json!({})).await;
        done.store(true, Ordering::SeqCst);
        began.elapsed()
    };
    let watch = async {
        let mut polls = Vec::new();
        for _ in 0..100 {
            let asked = began.elapsed();
            let found = logged(&w, "slow", "first").await.len() == 1;
            polls.push((asked, began.elapsed(), found));
            if found {
                return (!done.load(Ordering::SeqCst), polls);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        (false, polls)
    };
    let (ended, (seen_while_running, polls)) = tokio::join!(invoke, watch);
    assert!(
        seen_while_running,
        "the first line was written while the function ran: the invocation ended at {ended:?}; polls (asked, answered, found): {polls:?}"
    );
    let lines = logged(&w, "slow", "").await;
    assert_eq!(lines.len(), 5, "{lines:?}");
    assert!(lines[0].starts_with("START RequestId: "), "{lines:?}");
    assert_eq!(&lines[1..3], ["first", "second"]);
    assert!(lines[3].starts_with("END RequestId: ") && lines[4].starts_with("REPORT RequestId: "));
    w.stop().await;
}

/// Node's writes to a pipe are queued, not made at once (macOS). Before: the shim posted the answer with them still queued, and the
/// lines read after Lambda had taken the invocation's were not in its log (none of the four below were, in four runs of four). Now
/// every line the handler wrote is in its own invocation, before `END`.
#[tokio::test]
async fn every_line_a_node_handler_writes_is_in_its_own_invocation() {
    let node = std::env::var("DOZE_LAMBDA_NODE")
        .ok()
        .filter(|n| !n.is_empty());
    let runs = std::process::Command::new(node.as_deref().unwrap_or("node"))
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !runs {
        eprintln!("skipped: Node is not on PATH or in DOZE_LAMBDA_NODE");
        return;
    }
    let w = World::with_config(crate::Config {
        quiet: true,
        runtime: crate::RuntimeConfig {
            interpreters: crate::Interpreters {
                node,
                ..crate::Interpreters::default()
            },
            ..crate::RuntimeConfig::default()
        },
        ..crate::Config::default()
    });
    // Lines far longer than a pipe holds (64 KiB): Node writes what fits and queues the rest, which is still queued when the handler
    // returns. Four of 200 KB stay under what Lambda holds for an invocation (1 MiB).
    let code = b"export const handler = async () => {\n  for (let i = 0; i < 4; i++) console.log('chunk ' + i + ' ' + 'x'.repeat(200000));\n  return 1;\n};\n";
    w.create(
        "chatty",
        "nodejs22.x",
        "index.handler",
        &[("index.mjs", code, 0o644)],
    )
    .await;
    w.invoke("chatty", &json!({})).await;
    let lines = every_line(&w, "chatty").await;
    let end = lines.iter().position(|l| l.starts_with("END RequestId: "));
    let chunks: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.contains("\tchunk "))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(chunks.len(), 4, "every line is logged");
    assert!(
        end.is_some_and(|e| chunks.iter().all(|c| *c < e)),
        "every line comes before END: END at {end:?}, last line at {:?}",
        chunks.last()
    );
    w.stop().await;
}

/// Every line of `function`'s log, page by page (a page of FilterLogEvents holds 10,000 events or 1 MB).
async fn every_line(w: &World, function: &str) -> Vec<String> {
    logged(w, function, "START").await;
    let mut lines = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut ask = json!({"logGroupName": format!("/aws/lambda/{function}")});
        if let Some(t) = &token {
            ask["nextToken"] = json!(t);
        }
        let page = w.peer("logs", "FilterLogEvents", ask).await;
        for e in page["events"].as_array().into_iter().flatten() {
            lines.push(e["message"].as_str().unwrap_or_default().trim().to_owned());
        }
        match page["nextToken"].as_str() {
            Some(t) if token.as_deref() != Some(t) => token = Some(t.to_owned()),
            _ => return lines,
        }
    }
}
