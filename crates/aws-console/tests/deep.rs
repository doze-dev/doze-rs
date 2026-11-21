//! The deep pages' API against real services on a real bus, each call made as the caller `console`: a function invoked (its
//! answer, its error, its log tail) and configured; an execution started, read with its history, and stopped. A log
//! stream, a Live Tail and a stack are in `deep_logs.rs`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines
)]

mod support;
#[path = "resources/world.rs"]
mod world;

use std::time::Duration;

use aws_peers::Mode;
use serde_json::{Value, json};
use support::{keys, ts_keys};
use world::{World, field, world};

/// A `provided` function in sh over the Runtime API (needs `curl`): echoes its event, fails when asked.
const BOOTSTRAP: &str = r#"#!/bin/sh
while true; do
  H=$(mktemp)
  EVENT=$(curl -sS -D "$H" "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/next")
  ID=$(grep -i '^Lambda-Runtime-Aws-Request-Id' "$H" | cut -d: -f2 | tr -d ' \r\n')
  rm -f "$H"
  echo "handling $ID"
  case "$EVENT" in
    *fail*) curl -sS -X POST "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/$ID/error" -d '{"errorMessage":"asked to fail","errorType":"Custom"}' ;;
    *) curl -sS -X POST "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/$ID/response" -d "{\"echo\":$EVENT,\"mode\":\"$MODE\"}" ;;
  esac
done
"#;

fn have(program: &str) -> bool {
    std::process::Command::new("sh")
        .args(["-c", &format!("command -v {program}")])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// The bootstrap zipped (with the `zip` program, as a person would).
fn zipped() -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bootstrap");
    std::fs::write(&file, BOOTSTRAP).unwrap();
    std::process::Command::new("chmod")
        .args(["755", file.to_str().unwrap()])
        .status()
        .unwrap();
    let zip = dir.path().join("f.zip");
    let ok = std::process::Command::new("zip")
        .args(["-qj", zip.to_str().unwrap(), file.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(ok.success());
    std::fs::read(zip).unwrap()
}

fn b64(bytes: &[u8]) -> String {
    // The tests' own base64 (the console's is aws-core's): standard alphabet, padded.
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(char::from(A[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `operation` of a resource's pages (`lambda/functions/echo`), made through the console.
async fn op(w: &World, resource: &str, operation: &str, body: &Value) -> (u16, Value) {
    w.send(
        "POST",
        &format!("/api/v1/resources/{resource}/op/{operation}"),
        body,
    )
    .await
}

#[tokio::test]
async fn a_function_is_invoked_with_its_log_and_configured() {
    if !have("curl") || !have("zip") {
        return;
    }
    let w = world("deep-lambda", Mode::Enforce, &[]);
    let trust = json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Principal": {"Service": "lambda.amazonaws.com"}, "Action": "sts:AssumeRole"}]});
    w.app(
        "iam",
        "CreateRole",
        &json!({"RoleName": "r", "AssumeRolePolicyDocument": trust.to_string()}),
    )
    .await;
    w.app(
        "lambda",
        "CreateFunction",
        &json!({"FunctionName": "echo", "Runtime": "provided.al2023", "Handler": "x", "Role": "arn:aws:iam::000000000000:role/r",
            "Code": {"ZipFile": b64(&zipped())}, "Timeout": 10}),
    )
    .await;
    let (status, ok) = op(
        &w,
        "lambda/functions/echo",
        "Invoke",
        &json!({"FunctionName": "echo", "Payload": "{\"n\":1}", "LogType": "Tail"}),
    )
    .await;
    assert_eq!(status, 200, "{ok}");
    assert_eq!(keys(&ok), ts_keys("Invoked"));
    assert_eq!(ok["status"], 200);
    assert!(ok["function_error"].is_null());
    assert_eq!(ok["executed_version"], "$LATEST");
    let answered: Value = serde_json::from_str(ok["payload"].as_str().unwrap()).unwrap();
    assert_eq!(answered["echo"], json!({"n": 1}));
    let log = ok["log"].as_str().unwrap();
    assert!(
        log.contains("START RequestId") && log.contains("handling"),
        "{log}"
    );

    let (_, failed) = op(
        &w,
        "lambda/functions/echo",
        "Invoke",
        &json!({"FunctionName": "echo", "Payload": "{\"fail\":true}"}),
    )
    .await;
    assert_eq!(failed["function_error"], "Unhandled");
    assert!(
        failed["payload"]
            .as_str()
            .unwrap()
            .contains("asked to fail")
    );
    assert!(failed["log"].is_null());

    let (status, updated) = op(&w, "lambda/functions/echo", "UpdateFunctionConfiguration",
        &json!({"FunctionName": "echo", "MemorySize": 256, "Timeout": 5, "Environment": {"Variables": {"MODE": "blue"}}}),
    )
    .await;
    assert_eq!(status, 200, "{updated}");
    assert_eq!(updated["MemorySize"], 256);
    let (_, again) = op(
        &w,
        "lambda/functions/echo",
        "Invoke",
        &json!({"FunctionName": "echo"}),
    )
    .await;
    let answered: Value = serde_json::from_str(again["payload"].as_str().unwrap()).unwrap();
    assert_eq!(answered["mode"], "blue");
    let (_, versions) = op(
        &w,
        "lambda/functions/echo",
        "ListVersionsByFunction",
        &json!({"FunctionName": "echo"}),
    )
    .await;
    assert_eq!(versions["Versions"][0]["Version"], "$LATEST");
    let (_, aliases) = op(
        &w,
        "lambda/functions/echo",
        "ListAliases",
        &json!({"FunctionName": "echo"}),
    )
    .await;
    assert!(
        aliases["Aliases"].as_array().is_none_or(Vec::is_empty),
        "{aliases}"
    );
    // The function is the address's: one the page names is set aside.
    let (status, bound) = op(
        &w,
        "lambda/functions/echo",
        "Invoke",
        &json!({"FunctionName": "nope", "Payload": "{\"n\":2}"}),
    )
    .await;
    assert_eq!(status, 200, "{bound}");
    assert!(bound["payload"].as_str().unwrap().contains("\"n\":2"));
    // Lambda's own refusal, in its words; and the console's for an operation no page asks for.
    let (status, missing) = op(&w, "lambda/functions/nope", "Invoke", &json!({})).await;
    assert_eq!(
        (status, missing["code"].as_str()),
        (404, Some("ResourceNotFoundException"))
    );
    let (status, _) = op(
        &w,
        "lambda/functions/echo",
        "DeleteFunction",
        &json!({"FunctionName": "echo"}),
    )
    .await;
    assert_eq!(status, 404);
    let live = w.live();
    assert!(
        live.contains(&"console lambda Invoke".to_owned()),
        "{live:?}"
    );
    assert!(live.contains(&"console lambda UpdateFunctionConfiguration".to_owned()));
    // The detail page names its pages.
    let (_, detail) = w.get("/api/v1/resources/lambda/functions/echo").await;
    let tabs: Vec<&str> = detail["deep"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        tabs,
        [
            "code",
            "test",
            "monitor",
            "configuration",
            "aliases",
            "versions"
        ]
    );
    assert_eq!(field(&detail, "Memory (MB)")["value"], "256");
}

#[tokio::test]
async fn an_execution_is_started_read_and_stopped() {
    let w = world("deep-states", Mode::Soft, &[]);
    let definition = json!({"StartAt": "Pick", "States": {
        "Pick": {"Type": "Choice", "Choices": [{"Variable": "$.wait", "IsPresent": true, "Next": "Hold"}], "Default": "Done"},
        "Hold": {"Type": "Wait", "Seconds": 60, "Next": "Done"},
        "Done": {"Type": "Pass", "End": true}}});
    let made = w
        .app(
            "states",
            "CreateStateMachine",
            &json!({"name": "flow", "definition": definition.to_string(), "roleArn": "arn:aws:iam::000000000000:role/sfn"}),
        )
        .await;
    let machine = made["stateMachineArn"].as_str().unwrap();
    let (status, quick) = op(
        &w,
        "stepfunctions/machines/flow",
        "StartExecution",
        &json!({"stateMachineArn": machine, "name": "quick", "input": "{}"}),
    )
    .await;
    assert_eq!(status, 200, "{quick}");
    let quick = quick["executionArn"].as_str().unwrap().to_owned();
    let mut done = Value::Null;
    for _ in 0..50 {
        done = op(
            &w,
            "stepfunctions/machines/flow",
            "DescribeExecution",
            &json!({"executionArn": quick}),
        )
        .await
        .1;
        if done["status"] == "SUCCEEDED" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(done["status"], "SUCCEEDED", "{done}");
    let (_, history) = op(
        &w,
        "stepfunctions/machines/flow",
        "GetExecutionHistory",
        &json!({"executionArn": quick, "reverseOrder": true, "maxResults": 100}),
    )
    .await;
    let types: Vec<&str> = history["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["type"].as_str().unwrap())
        .collect();
    assert_eq!(types.first(), Some(&"ExecutionSucceeded"), "{types:?}");
    assert!(types.contains(&"ChoiceStateEntered") && !types.contains(&"WaitStateEntered"));

    let (_, slow) = op(
        &w,
        "stepfunctions/machines/flow",
        "StartExecution",
        &json!({"stateMachineArn": machine, "name": "slow", "input": "{\"wait\":true}"}),
    )
    .await;
    let slow = slow["executionArn"].as_str().unwrap().to_owned();
    let (status, stopped) = op(
        &w,
        "stepfunctions/machines/flow",
        "StopExecution",
        &json!({"executionArn": slow, "error": "Console", "cause": "stopped from its page"}),
    )
    .await;
    assert_eq!(status, 200, "{stopped}");
    let (_, read) = op(
        &w,
        "stepfunctions/machines/flow",
        "DescribeExecution",
        &json!({"executionArn": slow}),
    )
    .await;
    assert_eq!(read["status"], "ABORTED");
    assert!(
        w.live()
            .contains(&"console states StopExecution".to_owned())
    );
    // An execution page acts only on an execution of the machine in its address: not another machine's, nor none.
    w.app(
        "states",
        "CreateStateMachine",
        &json!({"name": "flow-2", "definition": definition.to_string(), "roleArn": "arn:aws:iam::000000000000:role/sfn"}),
    )
    .await;
    for (machine, asked) in [
        ("flow-2", json!({"executionArn": quick})),
        (
            "flow",
            json!({"executionArn": quick.replace(":flow:", ":flow-2:")}),
        ),
        ("flow", json!({})),
    ] {
        let (status, refused) = op(
            &w,
            &format!("stepfunctions/machines/{machine}"),
            "DescribeExecution",
            &asked,
        )
        .await;
        assert_eq!(
            (status, refused["code"].as_str()),
            (400, Some("NOT_THIS_RESOURCE")),
            "{machine} {asked}"
        );
    }
    let (_, tabs) = w.get("/api/v1/resources/stepfunctions/machines/flow").await;
    assert!(
        tabs["subs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["title"] != "Executions"),
        "the executions are a tab, not a list on the details: {tabs}"
    );
}
