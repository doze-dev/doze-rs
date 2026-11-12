//! Running functions: a shell `bootstrap` (the `provided` runtime, which needs only `curl`), Python through the embedded client, a
//! function error, a timeout, and no process left behind.

use serde_json::json;

use super::{World, have};

/// A `provided` function: a loop over the Runtime API in sh, which echoes its event and its environment.
const BOOTSTRAP: &[u8] = br#"#!/bin/sh
while true; do
  H=$(mktemp)
  EVENT=$(curl -sS -D "$H" "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/next")
  ID=$(grep -i '^Lambda-Runtime-Aws-Request-Id' "$H" | cut -d: -f2 | tr -d ' \r\n')
  rm -f "$H"
  echo "handling $ID"
  case "$EVENT" in
    *fail*) curl -sS -X POST "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/$ID/error" -d '{"errorMessage":"asked to fail","errorType":"Custom"}' ;;
    *sleep*) sleep 30 ;;
    *) curl -sS -X POST "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/$ID/response" -d "{\"echo\":$EVENT,\"name\":\"$AWS_LAMBDA_FUNCTION_NAME\",\"hello\":\"$HELLO\",\"region\":\"$AWS_REGION\"}" ;;
  esac
done
"#;

#[tokio::test]
async fn a_provided_function_answers_and_fails_and_times_out() {
    if !have("curl") || !have("sh") {
        return;
    }
    let w = World::new();
    w.create(
        "sh",
        "provided.al2023",
        "bootstrap",
        &[("bootstrap", BOOTSTRAP, 0o755)],
    )
    .await;
    let ok = w.invoke("sh", &json!({"n": 1})).await;
    assert_eq!(ok["StatusCode"], 200);
    assert_eq!(ok["ExecutedVersion"], "$LATEST");
    assert_eq!(ok["Payload"]["echo"], json!({"n": 1}));
    assert_eq!(ok["Payload"]["name"], "sh");
    assert_eq!(ok["Payload"]["region"], "us-east-1");

    let failed = w.invoke("sh", &json!({"fail": true})).await;
    assert_eq!(failed["FunctionError"], "Unhandled");
    assert_eq!(failed["Payload"]["errorMessage"], "asked to fail");

    // The process that failed is the one that answers the next call: a handled error does not end it.
    assert_eq!(
        w.invoke("sh", &json!({"n": 2})).await["Payload"]["echo"],
        json!({"n": 2})
    );

    w.ok("UpdateFunctionConfiguration", json!({"FunctionName": "sh", "Timeout": 1, "Environment": {"Variables": {"HELLO": "there"}}})).await;
    assert_eq!(
        w.invoke("sh", &json!({"n": 3})).await["Payload"]["hello"],
        "there"
    );
    let slow = w.invoke("sh", &json!({"sleep": true})).await;
    assert_eq!(slow["FunctionError"], "Unhandled");
    let msg = slow["Payload"]["errorMessage"].as_str().unwrap();
    assert!(msg.ends_with("Task timed out after 1.00 seconds"), "{msg}");
    w.stop().await;
}

#[tokio::test]
async fn a_python_function_runs_through_the_embedded_client() {
    if !have("python3") {
        return;
    }
    let w = World::new();
    let source = b"import os\ndef handler(event, context):\n    if event.get('fail'):\n        raise ValueError('asked to fail')\n    return {'echo': event, 'fn': context.function_name, 'version': context.function_version}\n";
    w.create(
        "py",
        "python3.12",
        "index.handler",
        &[("index.py", source, 0o644)],
    )
    .await;
    let ok = w.invoke("py", &json!({"n": 1})).await;
    assert_eq!(
        ok["Payload"],
        json!({"echo": {"n": 1}, "fn": "py", "version": "$LATEST"})
    );
    let bad = w.invoke("py", &json!({"fail": true})).await;
    assert_eq!(bad["FunctionError"], "Unhandled");
    assert_eq!(bad["Payload"]["errorType"], "ValueError");
    assert_eq!(bad["Payload"]["errorMessage"], "asked to fail");
    w.stop().await;
}

#[tokio::test]
async fn shutdown_leaves_no_function_process_behind() {
    if !have("curl") || !have("sh") {
        return;
    }
    let w = World::new();
    w.create(
        "sh",
        "provided.al2023",
        "bootstrap",
        &[("bootstrap", BOOTSTRAP, 0o755)],
    )
    .await;
    w.invoke("sh", &json!({})).await;
    let tasks = w.tasks.clone();
    tasks.shutdown().await;
    assert!(tasks.is_empty(), "tracked tasks are left: {}", tasks.len());
}

#[tokio::test]
async fn a_command_replaces_the_launch_line_and_layers_go_on_the_search_path() {
    if !have("curl") || !have("sh") || !have("python3") {
        return;
    }
    let w = World::new();
    let dir = std::env::temp_dir().join(format!("doze-lambda-command-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("run.sh");
    std::fs::write(&script, BOOTSTRAP).unwrap();
    let zip = crate::zipread::build(&[("unused", b"x", 0o644)]);
    w.ok(
        "CreateFunction",
        json!({"FunctionName": "cmd", "Runtime": "provided.al2023", "Handler": "x", "Role": "arn:aws:iam::111122223333:role/r",
            "Code": {"ZipFile": aws_core::base64::encode(&zip)}, "Command": ["sh", script.to_string_lossy()]}),
    )
    .await;
    assert_eq!(
        w.invoke("cmd", &json!({"n": 1})).await["Payload"]["echo"],
        json!({"n": 1})
    );

    // A layer's `python/` directory is on the Python path.
    let layer = crate::zipread::build(&[("python/helper.py", b"VALUE = 'from the layer'", 0o644)]);
    let published = w
        .ok(
            "PublishLayerVersion",
            json!({"LayerName": "lib", "Content": {"ZipFile": aws_core::base64::encode(&layer)}}),
        )
        .await;
    let source = b"import helper\ndef handler(e, c):\n    return helper.VALUE\n";
    let code = crate::zipread::build(&[("index.py", source, 0o644)]);
    w.ok(
        "CreateFunction",
        json!({"FunctionName": "withlayer", "Runtime": "python3.12", "Handler": "index.handler", "Role": "arn:aws:iam::111122223333:role/r",
            "Code": {"ZipFile": aws_core::base64::encode(&code)}, "Layers": [published["LayerVersionArn"]]}),
    )
    .await;
    assert_eq!(
        w.invoke("withlayer", &json!({})).await["Payload"],
        "from the layer"
    );
    drop(std::fs::remove_dir_all(&dir));
    w.stop().await;
}
