//! The hook for the Lab's rules, over a real socket with a fake SQS behind it and a scripted table: a request is judged after it is decoded and before the service; a fail answers in the protocol's own shape and the service never sees the request; a request no rule touches is untouched.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod fault;

use std::sync::atomic::Ordering;

use aws_gateway::{Done, Failure, Verdict};
use fault::{SEND, Script, send_raw, start, tag, text, throttle};

#[tokio::test]
async fn a_request_is_judged_with_what_the_pipeline_learnt_of_it() {
    let script = Script::new(Verdict::Pass);
    let h = start(Some(script.clone()), true).await;
    let answer = text(&send_raw(h.addr, SEND).await);
    assert!(answer.starts_with("http/1.1 200"), "{answer}");
    assert_eq!(
        *script.seen.lock().unwrap(),
        ["sqs|SQS|SendMessage|http://localhost:4566/000000000000/orders|AKIDLAB|eu-west-1"]
    );
    assert_eq!(h.service.0.load(Ordering::SeqCst), 1);
    let told = h.tape.0.lock().unwrap().clone();
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].lab, None, "a request no rule touched has no tag");
    h.running.shutdown().await;
}

#[tokio::test]
async fn a_fail_answers_in_the_protocols_shape_and_the_service_never_sees_the_request() {
    let script = Script::new(throttle());
    let h = start(Some(script), true).await;
    let answer = text(&send_raw(h.addr, SEND).await);
    // SQS's JSON protocol, with its Query-compatible code in the header SDKs read it from, and the model's status for the error.
    assert!(answer.starts_with("http/1.1 403"), "{answer}");
    assert!(
        answer.contains("x-amzn-query-error: requestthrottled;sender"),
        "{answer}"
    );
    assert!(answer.contains("requestthrottled"), "{answer}");
    assert!(answer.contains("rate exceeded."), "{answer}");
    assert_eq!(
        h.service.0.load(Ordering::SeqCst),
        0,
        "a fault never reaches the service"
    );
    let told = h.tape.0.lock().unwrap().clone();
    assert_eq!(
        (told[0].status, told[0].code.as_str()),
        (403, "RequestThrottled")
    );
    assert_eq!(
        told[0].lab,
        Some((7, "throttle orders".to_owned(), Done::Failed))
    );
    h.running.shutdown().await;
}

#[tokio::test]
async fn a_fail_with_a_code_the_model_does_not_carry_states_its_own_status_and_code() {
    let script = Script::new(Verdict::Fail(Box::new(Failure {
        shape: None,
        status: 503,
        code: "ServiceUnavailable".into(),
        message: "The service is temporarily unavailable.".to_owned(),
        tag: tag(),
    })));
    let h = start(Some(script), true).await;
    let answer = text(&send_raw(h.addr, SEND).await);
    assert!(answer.starts_with("http/1.1 503"), "{answer}");
    assert!(
        answer.contains("x-amzn-query-error: serviceunavailable;receiver"),
        "{answer}"
    );
    assert!(
        answer.contains("com.amazon.coral.service#serviceunavailable"),
        "{answer}"
    );
    assert_eq!(h.service.0.load(Ordering::SeqCst), 0);
    h.running.shutdown().await;
}
