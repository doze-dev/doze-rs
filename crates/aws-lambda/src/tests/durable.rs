//! Durable functions end to end: a Python function that speaks the durable execution protocol by hand (the standard library only,
//! through a real gateway), so the checkpoints, the replay payload, the wait, the callback and the re-invocations are the ones a
//! durable SDK sees; and, when the host has it, AWS's own JavaScript SDK (`@aws/durable-execution-sdk-js`).

use std::time::Duration;

use aws_core::base64;
use serde_json::{Value, json};

use super::{ACCOUNT, World, have};
use crate::Config;

/// A step, a wait of a second, then a callback, as the SDK would checkpoint them.
const HANDLER: &[u8] = br#"
import datetime, json, os, urllib.parse, urllib.request

def call(method, path, body=None):
    now = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    req = urllib.request.Request(os.environ["AWS_ENDPOINT_URL"] + path, method=method,
                                 data=None if body is None else json.dumps(body).encode())
    req.add_header("Content-Type", "application/json")
    req.add_header("X-Amz-Date", now)
    scope = f"{os.environ['AWS_ACCESS_KEY_ID']}/{now[:8]}/{os.environ['AWS_REGION']}/lambda/aws4_request"
    req.add_header("Authorization", f"AWS4-HMAC-SHA256 Credential={scope}, SignedHeaders=host;x-amz-date, Signature={'0' * 64}")
    with urllib.request.urlopen(req) as r:
        return json.loads(r.read() or b"{}")

def handler(event, context):
    arn, token = event["DurableExecutionArn"], event["CheckpointToken"]
    ops = {o["Id"]: o for o in event["InitialExecutionState"]["Operations"]}
    path = "/2025-12-01/durable-executions/" + urllib.parse.quote(arn, safe="") + "/checkpoint"

    def checkpoint(updates):
        nonlocal token
        out = call("POST", path, {"CheckpointToken": token, "Updates": updates})
        token = out.get("CheckpointToken")
        for o in out["NewExecutionState"]["Operations"]:
            ops[o["Id"]] = o

    execution = next(o for o in ops.values() if o["Type"] == "EXECUTION")
    given = json.loads(execution["ExecutionDetails"]["InputPayload"])
    if "step1" not in ops:
        checkpoint([{"Id": "step1", "Type": "STEP", "Action": "START", "Name": "double", "SubType": "Step"},
                    {"Id": "step1", "Type": "STEP", "Action": "SUCCEED", "Payload": json.dumps(given["n"] * 2)}])
    doubled = json.loads(ops["step1"]["StepDetails"]["Result"])
    if "wait1" not in ops:
        checkpoint([{"Id": "wait1", "Type": "WAIT", "Action": "START", "WaitOptions": {"WaitSeconds": 1}}])
    if ops["wait1"]["Status"] != "SUCCEEDED":
        return {"Status": "PENDING"}
    if given.get("quick"):
        return {"Status": "SUCCEEDED", "Result": json.dumps({"doubled": doubled})}
    if "cb1" not in ops:
        checkpoint([{"Id": "cb1", "Type": "CALLBACK", "Action": "START", "Name": "approve",
                     "CallbackOptions": {"TimeoutSeconds": 60}}])
    cb = ops["cb1"]
    if cb["Status"] == "STARTED":
        return {"Status": "PENDING"}
    if cb["Status"] != "SUCCEEDED":
        return {"Status": "FAILED", "Error": cb["CallbackDetails"].get("Error", {})}
    approval = json.loads(cb["CallbackDetails"]["Result"])
    return {"Status": "SUCCEEDED", "Result": json.dumps({"doubled": doubled, "approval": approval})}
"#;

async fn durable_world() -> World {
    let mut w = World::with_config(Config {
        quiet: true,
        durable_wait_scale: Some(0.2),
        ..Config::default()
    });
    w.serve().await;
    w
}

async fn create(w: &World, name: &str, source: &[u8], durable: bool) {
    let zip = crate::zipread::build(&[("index.py", source, 0o644)]);
    let mut req = json!({"FunctionName": name, "Runtime": "python3.12", "Role": format!("arn:aws:iam::{ACCOUNT}:role/r"),
        "Handler": "index.handler", "Code": {"ZipFile": base64::encode(&zip)}, "Timeout": 10});
    if durable {
        req["DurableConfig"] = json!({"ExecutionTimeout": 120});
    }
    w.ok("CreateFunction", req).await;
}

async fn status(w: &World, arn: &str) -> Value {
    w.ok("GetDurableExecution", json!({"DurableExecutionArn": arn}))
        .await
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

async fn callback_of(w: &World, arn: &str) -> String {
    let mut id = String::new();
    for _ in 0..200 {
        let events = history(w, arn).await;
        if let Some(e) = events.iter().find(|e| e["EventType"] == "CallbackStarted") {
            id = e["CallbackStartedDetails"]["CallbackId"]
                .as_str()
                .unwrap()
                .to_owned();
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        !id.is_empty(),
        "the callback never started: {:?}",
        history(w, arn).await
    );
    id
}

async fn ended(w: &World, arn: &str) -> Value {
    for _ in 0..200 {
        let s = status(w, arn).await;
        if s["Status"] != "RUNNING" {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("never ended: {:?}", history(w, arn).await);
}

#[tokio::test]
async fn a_step_a_wait_and_a_callback_run_across_invocations() {
    if !have("python3") {
        return;
    }
    let w = durable_world().await;
    create(&w, "dur", HANDLER, true).await;
    let shown = w
        .ok("GetFunctionConfiguration", json!({"FunctionName": "dur"}))
        .await;
    assert_eq!(
        shown["DurableConfig"],
        json!({"ExecutionTimeout": 120, "RetentionPeriodInDays": 14})
    );

    let unqualified = w
        .err(
            "Invoke",
            json!({"FunctionName": "dur", "Payload": base64::encode(b"{}")}),
        )
        .await;
    assert_eq!(unqualified.code, "InvalidParameterValueException");

    let payload = base64::encode(br#"{"n": 21}"#);
    let started = w
        .ok("Invoke", json!({"FunctionName": "dur:$LATEST", "InvocationType": "Event", "DurableExecutionName": "order-1", "Payload": payload}))
        .await;
    assert_eq!(started["StatusCode"], 202);
    let arn = started["DurableExecutionArn"].as_str().unwrap().to_owned();
    assert!(
        arn.contains(":function:dur:$LATEST/durable-execution/order-1/"),
        "{arn}"
    );

    // The same name and payload is the same execution; another payload is refused.
    let again = w
        .ok("Invoke", json!({"FunctionName": "dur:$LATEST", "InvocationType": "Event", "DurableExecutionName": "order-1", "Payload": payload}))
        .await;
    assert_eq!(again["DurableExecutionArn"], json!(arn));
    let other = w
        .err("Invoke", json!({"FunctionName": "dur:$LATEST", "InvocationType": "Event", "DurableExecutionName": "order-1", "Payload": base64::encode(b"{\"n\": 1}")}))
        .await;
    assert_eq!(other.code, "DurableExecutionAlreadyStartedException");

    let callback = callback_of(&w, &arn).await;
    assert_eq!(status(&w, &arn).await["Status"], "RUNNING");
    w.ok(
        "SendDurableExecutionCallbackHeartbeat",
        json!({"CallbackId": callback}),
    )
    .await;
    w.ok(
        "SendDurableExecutionCallbackSuccess",
        json!({"CallbackId": callback, "Result": base64::encode(br#"{"ok": true}"#)}),
    )
    .await;
    let done = ended(&w, &arn).await;
    assert_eq!(done["Status"], "SUCCEEDED", "{done}");
    let result: Value = serde_json::from_str(done["Result"].as_str().unwrap()).unwrap();
    assert_eq!(result, json!({"doubled": 42, "approval": {"ok": true}}));
    assert_eq!(done["InputPayload"], r#"{"n": 21}"#);
    let late = w
        .err(
            "SendDurableExecutionCallbackSuccess",
            json!({"CallbackId": callback}),
        )
        .await;
    assert_eq!(late.code, "CallbackTimeoutException");

    let kinds: Vec<String> = history(&w, &arn)
        .await
        .iter()
        .map(|e| e["EventType"].as_str().unwrap().to_owned())
        .collect();
    for kind in [
        "ExecutionStarted",
        "StepSucceeded",
        "WaitStarted",
        "WaitSucceeded",
        "CallbackStarted",
        "CallbackSucceeded",
        "ExecutionSucceeded",
    ] {
        assert!(
            kinds.iter().any(|k| k == kind),
            "{kind} missing from {kinds:?}"
        );
    }
    assert_eq!(
        kinds.iter().filter(|k| *k == "InvocationCompleted").count(),
        3,
        "{kinds:?}"
    );
    assert_eq!(kinds.first().map(String::as_str), Some("ExecutionStarted"));
    assert_eq!(kinds.last().map(String::as_str), Some("ExecutionSucceeded"));

    let list = w
        .ok(
            "ListDurableExecutionsByFunction",
            json!({"FunctionName": "dur", "Statuses": ["SUCCEEDED"]}),
        )
        .await;
    assert_eq!(list["DurableExecutions"].as_array().unwrap().len(), 1);
    assert_eq!(
        list["DurableExecutions"][0]["DurableExecutionName"],
        "order-1"
    );
    w.stop().await;
}

#[tokio::test]
async fn a_synchronous_invoke_waits_for_the_execution_and_a_stopped_one_ends() {
    if !have("python3") {
        return;
    }
    let w = durable_world().await;
    create(&w, "dur", HANDLER, true).await;
    let reply = w
        .ok("Invoke", json!({"FunctionName": "dur:$LATEST", "Payload": base64::encode(br#"{"n": 1, "quick": true}"#)}))
        .await;
    assert_eq!(reply["StatusCode"], 200);
    assert!(reply.get("FunctionError").is_none(), "{reply}");
    let body = base64::decode(reply["Payload"].as_str().unwrap()).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        json!({"doubled": 2})
    );
    assert!(reply["DurableExecutionArn"].as_str().is_some());

    let started = w
        .ok("Invoke", json!({"FunctionName": "dur:$LATEST", "InvocationType": "Event", "Payload": base64::encode(br#"{"n": 2}"#)}))
        .await;
    let arn = started["DurableExecutionArn"].as_str().unwrap().to_owned();
    let callback = callback_of(&w, &arn).await;
    let stopped = w
        .ok(
            "StopDurableExecution",
            json!({"DurableExecutionArn": arn, "Error": {"ErrorMessage": "changed my mind"}}),
        )
        .await;
    assert!(stopped["StopTimestamp"].as_f64().is_some());
    let s = status(&w, &arn).await;
    assert_eq!(s["Status"], "STOPPED");
    assert_eq!(s["Error"]["ErrorMessage"], "changed my mind");
    assert_eq!(
        w.err("StopDurableExecution", json!({"DurableExecutionArn": arn}))
            .await
            .code,
        "InvalidParameterValueException"
    );
    let late = w
        .err(
            "SendDurableExecutionCallbackFailure",
            json!({"CallbackId": callback, "Error": {"ErrorMessage": "no"}}),
        )
        .await;
    assert_eq!(late.code, "CallbackTimeoutException");

    // The executions count with the function's data.
    let usage = aws_wire::storage::Storage::usage(&w.lambda).await.unwrap();
    let dur = usage.resources.iter().find(|r| r.name == "dur").unwrap();
    assert!(
        dur.notes
            .iter()
            .any(|n| n.text.starts_with("in 2 durable executions")),
        "{:?}",
        dur.notes
    );
    w.stop().await;
}

#[tokio::test]
async fn durability_is_chosen_when_a_function_is_made() {
    if !have("python3") {
        return;
    }
    let w = World::new();
    create(&w, "plain", HANDLER, false).await;
    let e = w
        .err(
            "UpdateFunctionConfiguration",
            json!({"FunctionName": "plain", "DurableConfig": {"ExecutionTimeout": 60}}),
        )
        .await;
    assert_eq!(e.code, "InvalidParameterValueException");
    let zip = crate::zipread::build(&[("index.py", HANDLER, 0o644)]);
    let e = w
        .err("CreateFunction", json!({"FunctionName": "no-timeout", "Runtime": "python3.12", "Role": format!("arn:aws:iam::{ACCOUNT}:role/r"),
            "Handler": "index.handler", "Code": {"ZipFile": base64::encode(&zip)}, "DurableConfig": {"RetentionPeriodInDays": 3}}))
        .await;
    assert_eq!(e.code, "InvalidParameterValueException");
    let missing = w
        .err("GetDurableExecution", json!({"DurableExecutionArn": format!("arn:aws:lambda:us-east-1:{ACCOUNT}:function:plain:$LATEST/durable-execution/x/y")}))
        .await;
    assert_eq!(missing.code, "ResourceNotFoundException");
    w.stop().await;
}
