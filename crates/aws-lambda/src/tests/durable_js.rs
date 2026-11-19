//! A durable function written with AWS's JavaScript SDK (`@aws/durable-execution-sdk-js`), run unchanged: steps, a child context,
//! a map, a wait, and a callback answered from outside. The SDK is not vendored: the test runs when `DOZE_DURABLE_SDK_JS` names a
//! `node_modules` directory holding it (`npm install @aws/durable-execution-sdk-js` puts `@aws-sdk/client-lambda` beside it) and
//! Node 22 or later is `node` on `PATH` or `DOZE_LAMBDA_NODE`; otherwise it says so and passes.

use std::time::Duration;

use aws_core::base64;
use serde_json::{Value, json};

use super::{ACCOUNT, World};
use crate::{Config, Interpreters, RuntimeConfig};

const HANDLER: &[u8] = br#"
const { withDurableExecution } = require("@aws/durable-execution-sdk-js");

exports.handler = withDurableExecution(async (event, context) => {
  const doubled = await context.step("double", async () => event.n * 2);
  const squares = await context.map("squares", [1, 2, 3], async (ctx, item) => ctx.step(async () => item * item));
  const label = await context.runInChildContext("label", async (child) => child.step("name", async () => `order-${event.n}`));
  const flaky = await context.step("flaky", async () => {
    const fs = require("fs");
    if (!fs.existsSync(process.env.FLAKY_FILE)) {
      fs.writeFileSync(process.env.FLAKY_FILE, "1");
      throw new Error("first attempt fails");
    }
    return "recovered";
  }, { retryStrategy: (error, attempt) => ({ shouldRetry: attempt < 3, delay: { seconds: 1 } }) });
  const helped = await context.invoke("help", "helper", { x: 1 });
  await context.wait("pause", { seconds: 2 });
  const approval = await context.waitForCallback("approve", async () => {}, { timeout: { seconds: 60 } });
  return { doubled, squares: squares.getResults(), label, flaky, helped, approval };
});
"#;

fn sdk() -> Option<(String, Option<String>)> {
    let modules = std::env::var("DOZE_DURABLE_SDK_JS").ok().filter(|d| {
        std::path::Path::new(d)
            .join("@aws/durable-execution-sdk-js")
            .is_dir()
    });
    let node = std::env::var("DOZE_LAMBDA_NODE")
        .ok()
        .filter(|n| !n.is_empty());
    let runs = std::process::Command::new(node.as_deref().unwrap_or("node"))
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    match modules {
        Some(m) if runs => Some((m, node)),
        _ => {
            eprintln!(
                "skipped: set DOZE_DURABLE_SDK_JS to a node_modules holding @aws/durable-execution-sdk-js, with Node 22+ on PATH or in DOZE_LAMBDA_NODE"
            );
            None
        }
    }
}

#[tokio::test]
async fn the_javascript_sdk_runs_unchanged() {
    let Some((modules, node)) = sdk() else { return };
    let mut w = World::with_config(Config {
        quiet: true,
        durable_wait_scale: Some(0.5),
        runtime: RuntimeConfig {
            interpreters: Interpreters {
                node,
                ..Interpreters::default()
            },
            ..RuntimeConfig::default()
        },
    });
    w.serve().await;
    let flaky = w.dir().join("flaky").to_string_lossy().into_owned();
    let helper = crate::zipread::build(&[(
        "index.js",
        b"exports.handler = async (e) => ({ y: e.x + 1 });",
        0o644,
    )]);
    w.ok(
        "CreateFunction",
        json!({"FunctionName": "helper", "Runtime": "nodejs22.x", "Role": format!("arn:aws:iam::{ACCOUNT}:role/r"),
            "Handler": "index.handler", "Code": {"ZipFile": base64::encode(&helper)}}),
    )
    .await;
    let zip = crate::zipread::build(&[("index.js", HANDLER, 0o644)]);
    w.ok(
        "CreateFunction",
        json!({"FunctionName": "js", "Runtime": "nodejs22.x", "Role": format!("arn:aws:iam::{ACCOUNT}:role/r"),
            "Handler": "index.handler", "Code": {"ZipFile": base64::encode(&zip)}, "Timeout": 30,
            "Environment": {"Variables": {"NODE_PATH": modules, "FLAKY_FILE": flaky}}, "DurableConfig": {"ExecutionTimeout": 300}}),
    )
    .await;
    let started = w
        .ok("Invoke", json!({"FunctionName": "js:$LATEST", "InvocationType": "Event", "Payload": base64::encode(br#"{"n": 7}"#)}))
        .await;
    let arn = started["DurableExecutionArn"].as_str().unwrap().to_owned();
    let mut callback = None;
    for _ in 0..300 {
        let events: Vec<Value> = history(&w, &arn).await;
        if let Some(e) = events.iter().find(|e| e["EventType"] == "CallbackStarted") {
            callback = e["CallbackStartedDetails"]["CallbackId"]
                .as_str()
                .map(str::to_owned);
            break;
        }
        let state = w
            .ok("GetDurableExecution", json!({"DurableExecutionArn": arn}))
            .await;
        assert_eq!(state["Status"], "RUNNING", "{state} {events:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let callback = callback.expect("the SDK started a callback");
    w.ok(
        "SendDurableExecutionCallbackSuccess",
        json!({"CallbackId": callback, "Result": base64::encode(b"yes")}),
    )
    .await;
    let mut done = Value::Null;
    for _ in 0..300 {
        done = w
            .ok("GetDurableExecution", json!({"DurableExecutionArn": arn}))
            .await;
        if done["Status"] != "RUNNING" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        done["Status"],
        "SUCCEEDED",
        "{done} {:?}",
        history(&w, &arn).await
    );
    let result: Value = serde_json::from_str(done["Result"].as_str().unwrap()).unwrap();
    assert_eq!(
        result,
        json!({"doubled": 14, "squares": [1, 4, 9], "label": "order-7", "flaky": "recovered", "helped": {"y": 2}, "approval": "yes"})
    );
    let kinds: Vec<Value> = history(&w, &arn)
        .await
        .iter()
        .map(|e| e["EventType"].clone())
        .collect();
    for kind in [
        "ContextStarted",
        "ContextSucceeded",
        "StepSucceeded",
        "WaitSucceeded",
        "CallbackSucceeded",
        "StepFailed",
        "ChainedInvokeSucceeded",
    ] {
        assert!(
            kinds.contains(&json!(kind)),
            "{kind} missing from {kinds:?}"
        );
    }
    w.stop().await;
}

async fn history(w: &World, arn: &str) -> Vec<Value> {
    let h = w
        .ok(
            "GetDurableExecutionHistory",
            json!({"DurableExecutionArn": arn}),
        )
        .await;
    h["Events"].as_array().cloned().unwrap_or_default()
}
