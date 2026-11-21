//! Step Functions' pages against real services on a real bus: a machine made in the studio (validated, with the role the studio
//! makes), its versions, aliases, and executions by version and alias, its delete and what the delete leaves, and activities.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines
)]

#[path = "resources/blocked.rs"]
mod blocked;
mod support;
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use blocked::{names, titles};
use serde_json::{Value, json};
use world::{World, field, world};

const BASE: &str = "/api/v1/resources/stepfunctions";

const DEFINITION: &str = r#"{"StartAt":"Hold","States":{"Hold":{"Type":"Wait","Seconds":300,"Next":"Done"},"Done":{"Type":"Pass","End":true}}}"#;

async fn op(w: &World, one: &str, operation: &str, body: &Value) -> (u16, Value) {
    w.send("POST", &format!("{BASE}/{one}/op/{operation}"), body)
        .await
}

/// A machine made as the studio makes one: the call is the kind's own, `CreateStateMachine`.
async fn machine(w: &World, name: &str) -> String {
    let (status, made) = op(
        w,
        "machines",
        "CreateStateMachine",
        &json!({"name": name, "definition": DEFINITION, "roleArn": "arn:aws:iam::000000000000:role/sfn"}),
    )
    .await;
    assert_eq!(status, 200, "{made}");
    made["stateMachineArn"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn the_studio_checks_a_definition_makes_a_role_and_makes_the_machine() {
    let w = world("sfn-studio", Mode::Enforce, &[]);
    let (status, bad) = op(
        &w,
        "machines",
        "ValidateStateMachineDefinition",
        &json!({"definition": r#"{"StartAt":"A","States":{"A":{"Type":"Pass","Next":"Missing"}}}"#}),
    )
    .await;
    assert_eq!(status, 200, "{bad}");
    assert_eq!(bad["result"], "FAIL", "{bad}");
    assert!(!bad["diagnostics"].as_array().unwrap().is_empty());
    let (_, good) = op(
        &w,
        "machines",
        "ValidateStateMachineDefinition",
        &json!({"definition": DEFINITION}),
    )
    .await;
    assert_eq!(good["result"], "OK", "{good}");
    // Create new role: the role first (IAM's call, the console's), its permissions, then the machine that names it.
    let trust = json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow",
        "Principal": {"Service": "states.amazonaws.com"}, "Action": "sts:AssumeRole"}]});
    let (status, role) = op(
        &w,
        "machines",
        "CreateRole",
        &json!({"RoleName": "StepFunctions-stock-reorder-role", "AssumeRolePolicyDocument": trust.to_string()}),
    )
    .await;
    assert_eq!(status, 200, "{role}");
    let arn = role["Role"]["Arn"].as_str().unwrap().to_owned();
    let policy = json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow",
        "Action": "sqs:SendMessage", "Resource": "*"}]});
    let (status, put) = op(
        &w,
        "machines",
        "PutRolePolicy",
        &json!({"RoleName": "StepFunctions-stock-reorder-role", "PolicyName": "tasks", "PolicyDocument": policy.to_string()}),
    )
    .await;
    assert_eq!(status, 200, "{put}");
    // A log level needs a log group: the studio makes the one Step Functions names by default.
    let group = "/aws/vendedlogs/states/stock-reorder-Logs";
    let (status, logs) = op(
        &w,
        "machines",
        "CreateLogGroup",
        &json!({"logGroupName": group}),
    )
    .await;
    assert_eq!(status, 200, "{logs}");
    let (status, made) = op(
        &w,
        "machines",
        "CreateStateMachine",
        &json!({"name": "stock-reorder", "definition": DEFINITION, "roleArn": arn, "type": "STANDARD",
            "loggingConfiguration": {"level": "ERROR", "includeExecutionData": false, "destinations": [
                {"cloudWatchLogsLogGroup": {"logGroupArn": format!("arn:aws:logs:us-east-1:000000000000:log-group:{group}:*")}}]},
            "tags": [{"key": "app", "value": "fulfilment"}], "publish": true, "versionDescription": "First release"}),
    )
    .await;
    assert_eq!(status, 200, "{made}");
    assert!(made["stateMachineVersionArn"].as_str().is_some(), "{made}");
    let (_, detail) = w.get(&format!("{BASE}/machines/stock-reorder")).await;
    assert_eq!(field(&detail, "IAM role ARN")["value"], arn.as_str());
    assert_eq!(field(&detail, "Type")["value"], "STANDARD");
    assert_eq!(field(&detail, "Creation date")["label"], "Creation date");
    let badges: Vec<&str> = detail["badges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["text"].as_str().unwrap())
        .collect();
    assert_eq!(badges, ["Standard", "Active"]);
    let verbs: Vec<&str> = detail["verbs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().unwrap())
        .collect();
    assert_eq!(verbs, ["edit", "start"]);
    // The calls are the console's own, in Live.
    let live = w.live();
    for call in [
        "console iam CreateRole",
        "console iam PutRolePolicy",
        "console states CreateStateMachine",
    ] {
        assert!(live.contains(&call.to_owned()), "{call} in {live:?}");
    }
    // Only the operations the kind lists.
    let (status, refused) = op(&w, "machines", "DeleteStateMachine", &json!({})).await;
    assert_eq!(
        (status, refused["code"].as_str()),
        (404, Some("NOT_IN_PAGE"))
    );
}

#[tokio::test]
async fn versions_and_aliases_are_made_read_and_started_from() {
    let w = world("sfn-versions", Mode::Soft, &[]);
    let arn = machine(&w, "order-fulfilment").await;
    machine(&w, "order").await;
    let one = "machines/order-fulfilment";
    let (status, v1) = op(
        &w,
        one,
        "PublishStateMachineVersion",
        &json!({"description": "First release"}),
    )
    .await;
    assert_eq!(status, 200, "{v1}");
    let version = v1["stateMachineVersionArn"].as_str().unwrap().to_owned();
    assert_eq!(version, format!("{arn}:1"));
    // The page's call names the machine itself: a body that names another is forced back to this one.
    let (_, listed) = op(
        &w,
        one,
        "ListStateMachineVersions",
        &json!({"stateMachineArn": "arn:aws:states:us-east-1:000000000000:stateMachine:order"}),
    )
    .await;
    assert_eq!(
        listed["stateMachineVersions"][0]["stateMachineVersionArn"],
        version.as_str()
    );
    // A version is read for its description, and only a version of this machine.
    let (status, described) = op(
        &w,
        one,
        "DescribeStateMachine",
        &json!({"stateMachineArn": version}),
    )
    .await;
    assert_eq!(status, 200, "{described}");
    assert_eq!(described["description"], "First release");
    for other in [
        arn.clone(),
        "arn:aws:states:us-east-1:000000000000:stateMachine:order:1".to_owned(),
        String::new(),
    ] {
        let (status, refused) = op(
            &w,
            one,
            "DescribeStateMachine",
            &json!({"stateMachineArn": other}),
        )
        .await;
        assert_eq!(
            (status, refused["code"].as_str()),
            (400, Some("NOT_THIS_RESOURCE")),
            "{other}"
        );
    }
    let (status, alias) = op(
        &w,
        one,
        "CreateStateMachineAlias",
        &json!({"name": "PROD", "description": "Production traffic",
            "routingConfiguration": [{"stateMachineVersionArn": version, "weight": 100}]}),
    )
    .await;
    assert_eq!(status, 200, "{alias}");
    let alias = alias["stateMachineAliasArn"].as_str().unwrap().to_owned();
    let (_, read) = op(
        &w,
        one,
        "DescribeStateMachineAlias",
        &json!({"stateMachineAliasArn": alias}),
    )
    .await;
    assert_eq!(read["description"], "Production traffic");
    assert_eq!(read["routingConfiguration"][0]["weight"], 100);
    // An execution starts by the machine, by a version, or by an alias of it, and by no other machine's.
    for target in [&arn, &version, &alias] {
        let (status, started) = op(
            &w,
            one,
            "StartExecution",
            &json!({"stateMachineArn": target, "input": "{}"}),
        )
        .await;
        assert_eq!(status, 200, "{target}: {started}");
    }
    for target in [
        "arn:aws:states:us-east-1:000000000000:stateMachine:order",
        "arn:aws:states:us-east-1:000000000000:stateMachine:order:1",
    ] {
        let (status, refused) = op(
            &w,
            one,
            "StartExecution",
            &json!({"stateMachineArn": target}),
        )
        .await;
        assert_eq!(
            (status, refused["code"].as_str()),
            (400, Some("NOT_THIS_RESOURCE")),
            "{target}"
        );
    }
    let (_, runs) = op(&w, one, "ListExecutions", &json!({})).await;
    let runs = runs["executions"].as_array().unwrap();
    assert_eq!(runs.len(), 3, "{runs:?}");
    assert!(
        runs.iter()
            .any(|r| r["stateMachineAliasArn"] == alias.as_str()
                && r["stateMachineVersionArn"] == version.as_str())
    );
    // A machine and what goes with it: its versions, aliases and running executions are named before it is deleted.
    let (_, b) = w
        .get(&format!("{BASE}/machines/order-fulfilment/blockers"))
        .await;
    assert_eq!(titles(&b), ["Versions", "Aliases", "Running executions"]);
    assert_eq!(names(&b, "Versions"), ["order-fulfilment:1"]);
    assert_eq!(names(&b, "Aliases"), ["order-fulfilment:PROD"]);
    assert_eq!(names(&b, "Running executions").len(), 3);
    assert!(b["kept"][0].as_str().unwrap().contains("running"));
    let (status, gone) = w
        .send(
            "DELETE",
            &format!("{BASE}/machines/order-fulfilment"),
            &json!({}),
        )
        .await;
    assert_eq!(status, 200, "{gone}");
    let (status, missing) = w.get(&format!("{BASE}/machines/order-fulfilment")).await;
    assert_eq!(
        (status, missing["code"].as_str()),
        (400, Some("StateMachineDoesNotExist")),
        "{missing}"
    );
}

#[tokio::test]
async fn an_execution_is_redriven_only_on_its_own_machine() {
    let w = world("sfn-redrive", Mode::Soft, &[]);
    let fail = r#"{"StartAt":"Boom","States":{"Boom":{"Type":"Fail","Error":"KeyError","Cause":"'email'"}}}"#;
    for name in ["a", "b"] {
        let (status, made) = op(
            &w,
            "machines",
            "CreateStateMachine",
            &json!({"name": name, "definition": fail, "roleArn": "arn:aws:iam::000000000000:role/sfn"}),
        )
        .await;
        assert_eq!(status, 200, "{made}");
    }
    let (_, started) = op(
        &w,
        "machines/a",
        "StartExecution",
        &json!({"stateMachineArn": "arn:aws:states:us-east-1:000000000000:stateMachine:a", "name": "ord-1", "input": "{}"}),
    )
    .await;
    let run = started["executionArn"].as_str().unwrap().to_owned();
    let mut status = String::new();
    for _ in 0..50 {
        let (_, d) = op(
            &w,
            "machines/a",
            "DescribeExecution",
            &json!({"executionArn": run}),
        )
        .await;
        status = d["status"].as_str().unwrap().to_owned();
        if status == "FAILED" {
            assert_eq!(d["error"], "KeyError");
            assert_eq!(d["redriveStatus"], "REDRIVABLE", "{d}");
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(status, "FAILED");
    let (code, refused) = op(
        &w,
        "machines/b",
        "RedriveExecution",
        &json!({"executionArn": run}),
    )
    .await;
    assert_eq!(
        (code, refused["code"].as_str()),
        (400, Some("NOT_THIS_RESOURCE"))
    );
    let (code, done) = op(
        &w,
        "machines/a",
        "RedriveExecution",
        &json!({"executionArn": run}),
    )
    .await;
    assert_eq!(code, 200, "{done}");
    assert!(
        done["redriveDate"].is_number() || done["redriveDate"].is_string(),
        "{done}"
    );
}

#[tokio::test]
async fn an_activity_is_listed_made_read_and_deleted() {
    let w = world("sfn-activities", Mode::Soft, &[]);
    let (status, made) = op(
        &w,
        "activities",
        "CreateActivity",
        &json!({"name": "pack-order"}),
    )
    .await;
    assert_eq!(status, 200, "{made}");
    let arn = made["activityArn"].as_str().unwrap();
    assert!(arn.ends_with(":activity:pack-order"));
    let (_, list) = w.get(&format!("{BASE}/activities")).await;
    let row = &list["rows"][0];
    assert_eq!(row["name"], "pack-order");
    assert_eq!(row["id"], arn);
    let (_, detail) = w.get(&format!("{BASE}/activities/pack-order")).await;
    assert_eq!(field(&detail, "ARN")["value"], arn);
    let (status, gone) = w
        .send(
            "DELETE",
            &format!("{BASE}/activities/pack-order"),
            &json!({}),
        )
        .await;
    assert_eq!(status, 200, "{gone}");
    let (_, list) = w.get(&format!("{BASE}/activities")).await;
    assert!(list["rows"].as_array().unwrap().is_empty());
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
