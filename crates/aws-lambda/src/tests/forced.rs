//! Forced events (the console's Lab): a function's next invocation is a cold one, by the call that ends its warm processes.

use aws_peers::Mode;
use aws_wire::forced::{Step, kind};
use aws_wire::service::{Caller, Service};
use serde_json::{Value, json};

use super::behaviour::{TRUST, create_role, function_with_role};
use super::{World, have};

/// A Python function that answers with its own process id.
const PID_PY: &[u8] = b"import os\ndef handler(event, context):\n    return {'pid': os.getpid()}\n";

/// A `provided` function that answers with its own process id.
const WHO: &[u8] = br#"#!/bin/sh
while true; do
  H=$(mktemp)
  curl -sS -D "$H" "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/next" > /dev/null
  ID=$(grep -i '^Lambda-Runtime-Aws-Request-Id' "$H" | cut -d: -f2 | tr -d ' \r\n')
  rm -f "$H"
  curl -sS -X POST "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/$ID/response" -d "{\"pid\":$$}"
done
"#;

async fn pid(w: &World, name: &str) -> i64 {
    w.invoke(name, &json!({})).await["Payload"]["pid"]
        .as_i64()
        .unwrap()
}

async fn force(w: &World, who: &Caller, name: &str) -> Result<Step, aws_wire::ApiError> {
    w.lambda
        .forced()
        .unwrap()
        .force(who, kind::COLD_START, &json!({"function": name}))
        .await
}

#[tokio::test]
async fn a_forced_cold_start_ends_the_warm_process_and_the_next_invocation_starts_another() {
    if !have("curl") || !have("sh") {
        return;
    }
    let w = World::new();
    w.create(
        "who",
        "provided.al2023",
        "bootstrap",
        &[("bootstrap", WHO, 0o755)],
    )
    .await;
    let first = pid(&w, "who").await;
    assert_eq!(
        pid(&w, "who").await,
        first,
        "the second invocation is a warm one"
    );
    let before = w
        .ok("GetFunctionConfiguration", json!({"FunctionName": "who"}))
        .await;

    let step = force(&w, &World::caller(), "who").await.unwrap();
    let Step::Done(d) = step else {
        panic!("it should have ended the process")
    };
    assert_eq!(d.subject, "who");
    assert!(
        d.sentence.starts_with("Ended its 1 warm process of who:"),
        "{}",
        d.sentence
    );
    assert!(d.sentence.contains("Init Duration"), "{}", d.sentence);

    let cold = pid(&w, "who").await;
    assert_ne!(cold, first, "a new process answered");
    assert_eq!(pid(&w, "who").await, cold, "and it is warm from then on");
    // Nothing about the function was changed: its code and configuration are as they were.
    let after = w
        .ok("GetFunctionConfiguration", json!({"FunctionName": "who"}))
        .await;
    assert_eq!(before, after, "no revision, no change to the function");
    w.stop().await;
}

#[tokio::test]
async fn a_function_with_no_process_is_already_cold_and_a_missing_one_is_refused() {
    let w = World::new();
    w.create(
        "idle",
        "provided.al2023",
        "bootstrap",
        &[("bootstrap", WHO, 0o755)],
    )
    .await;
    let step = force(&w, &World::caller(), "idle").await.unwrap();
    let Step::Nothing(why) = step else {
        panic!("there is no process to end")
    };
    assert!(
        why.starts_with("idle has no process running in us-east-1"),
        "{why}"
    );
    assert!(
        why.contains("its next invocation is already a cold one"),
        "{why}"
    );
    assert!(why.contains("they end after 5 minutes idle"), "{why}");

    let e = force(&w, &World::caller(), "nope").await.unwrap_err();
    assert_eq!(e.code, "ResourceNotFoundException");
    let e = w
        .lambda
        .forced()
        .unwrap()
        .force(&World::caller(), kind::COLD_START, &json!({}))
        .await
        .unwrap_err();
    assert_eq!(e.message, "The forced event needs a function.");
    let e = w
        .lambda
        .forced()
        .unwrap()
        .force(
            &World::caller(),
            kind::SWEEP_TTL,
            &json!({"function": "idle"}),
        )
        .await
        .unwrap_err();
    assert!(
        e.message.contains("Lambda has no forced event sweep-ttl"),
        "{}",
        e.message
    );
    w.stop().await;
}

#[tokio::test]
async fn a_caller_without_the_right_is_refused_and_the_process_stays() {
    if !have("python3") {
        return;
    }
    let w = World::with_mode(Mode::Enforce);
    let role = create_role(&w, "fn-role", TRUST).await;
    function_with_role(&w, "who", &role, PID_PY).await.unwrap();
    let first = pid(&w, "who").await;
    w.peer("iam", "CreateUser", json!({"UserName": "nobody"}))
        .await;
    let key: Value = w
        .peer("iam", "CreateAccessKey", json!({"UserName": "nobody"}))
        .await;
    let nobody = Caller {
        access_key_id: key["AccessKey"]["AccessKeyId"].as_str().unwrap().to_owned(),
        ..World::caller()
    };
    let e = force(&w, &nobody, "who").await.unwrap_err();
    assert!(e.code.contains("AccessDenied"), "{} {}", e.code, e.message);
    assert!(
        e.message.contains("lambda:UpdateFunctionConfiguration"),
        "{}",
        e.message
    );
    assert_eq!(pid(&w, "who").await, first, "the process was not ended");
    // The account's root may.
    assert!(matches!(
        force(&w, &World::caller(), "who").await.unwrap(),
        Step::Done(_)
    ));
    w.stop().await;
}
