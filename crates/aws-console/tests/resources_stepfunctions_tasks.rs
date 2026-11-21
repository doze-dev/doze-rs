//! What a Step Functions Task gets from a function it invokes, against real services on a real bus: the function's answer as `Payload`,
//! and, for one that throws, the error it names and its error document as the cause (so the execution page can say why it failed).
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

use aws_peers::Mode;
use serde_json::{Value, json};
use world::{World, world};

const BASE: &str = "/api/v1/resources/stepfunctions";

async fn op(w: &World, one: &str, operation: &str, body: &Value) -> (u16, Value) {
    w.send("POST", &format!("{BASE}/{one}/op/{operation}"), body)
        .await
}

/// A `provided` function in sh over the Runtime API (needs `curl` and `zip`): answers with its event, fails when asked.
const BOOTSTRAP: &str = r#"#!/bin/sh
while true; do
  H=$(mktemp)
  EVENT=$(curl -sS -D "$H" "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/next")
  ID=$(grep -i '^Lambda-Runtime-Aws-Request-Id' "$H" | cut -d: -f2 | tr -d ' \r\n')
  rm -f "$H"
  case "$EVENT" in
    *fail*) curl -sS -X POST "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/$ID/error" -d '{"errorMessage":"no email","errorType":"KeyError"}' ;;
    *) curl -sS -X POST "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/$ID/response" -d "{\"echo\":$EVENT}" ;;
  esac
done
"#;

fn have(program: &str) -> bool {
    std::process::Command::new("sh")
        .args(["-c", &format!("command -v {program}")])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// What a Task that invokes a function gets back: the function's answer as `Payload`, and, for a function that throws, the error it
/// named and its error document as the cause (so the execution page can say why it failed).
#[tokio::test]
async fn a_lambda_task_gets_the_functions_answer_and_its_error() {
    if !have("curl") || !have("zip") {
        return;
    }
    let w = world("sfn-lambda", Mode::Soft, &[]);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bootstrap");
    std::fs::write(&file, BOOTSTRAP).unwrap();
    std::process::Command::new("chmod")
        .args(["755", file.to_str().unwrap()])
        .status()
        .unwrap();
    let zip = dir.path().join("f.zip");
    assert!(
        std::process::Command::new("zip")
            .args(["-qj", zip.to_str().unwrap(), file.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );
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
            "Code": {"ZipFile": aws_core::base64::encode(&std::fs::read(zip).unwrap())}, "Timeout": 10}),
    )
    .await;
    let definition = r#"{"StartAt":"Call","States":{"Call":{"Type":"Task","Resource":"arn:aws:states:::lambda:invoke","Parameters":{"FunctionName":"echo","Payload.$":"$"},"End":true}}}"#;
    let arn = {
        let (_, m) = op(
            &w,
            "machines",
            "CreateStateMachine",
            &json!({"name": "call", "definition": definition, "roleArn": "arn:aws:iam::000000000000:role/sfn"}),
        )
        .await;
        m["stateMachineArn"].as_str().unwrap().to_owned()
    };
    let mut results = Vec::new();
    for input in [r#"{"n":1}"#, r#"{"fail":true}"#] {
        let (_, started) = op(
            &w,
            "machines/call",
            "StartExecution",
            &json!({"stateMachineArn": arn, "input": input}),
        )
        .await;
        let run = started["executionArn"].as_str().unwrap().to_owned();
        let mut done = Value::Null;
        for _ in 0..100 {
            done = op(
                &w,
                "machines/call",
                "DescribeExecution",
                &json!({"executionArn": run}),
            )
            .await
            .1;
            if done["status"] != "RUNNING" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        results.push(done);
    }
    assert_eq!(results[0]["status"], "SUCCEEDED", "{}", results[0]);
    let out: Value = serde_json::from_str(results[0]["output"].as_str().unwrap()).unwrap();
    assert_eq!(out["Payload"]["echo"]["n"], 1, "{out}");
    assert_eq!(results[1]["status"], "FAILED", "{}", results[1]);
    assert_eq!(results[1]["error"], "KeyError");
    assert!(
        results[1]["cause"].as_str().unwrap().contains("no email"),
        "{}",
        results[1]
    );
}
