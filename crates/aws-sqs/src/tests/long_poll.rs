//! Long polling: a receive that waits is answered as soon as a message arrives, and does not stop the service from taking one.

use std::time::Duration;

use aws_wire::service::Service;
use doze_runtime::{Ctx, Tasks};

use super::*;
use crate::Sqs;

fn service() -> Arc<Sqs> {
    let ctx = Ctx {
        dir: Box::leak(Box::new(TempDir::new())).0.clone(),
        idle: Duration::from_secs(3600),
        tasks: Tasks::new(),
        clock: Arc::new(doze_runtime::SystemClock),
    };
    Arc::new(Sqs::new(Arc::new(ctx)))
}

async fn run(sqs: &Sqs, patterns: &Patterns, op: Operation, request: Value) -> Value {
    let call = call_of(patterns, op, &request).unwrap();
    let out = sqs
        .call(call)
        .await
        .unwrap_or_else(|e| panic!("{op:?}: {} {}", e.code, e.message));
    output_json(op, &out)
}

/// A receive that waits does not stop the service from taking a send, and returns as soon as the send lands.
// Real time: the writer is a thread, and a paused clock would jump ahead while the service waits for it.
#[tokio::test]
async fn a_waiting_receive_returns_when_a_message_arrives() {
    let (sqs, patterns) = (service(), Patterns::new(&TABLES));
    let url = run(
        &sqs,
        &patterns,
        Operation::CreateQueue,
        json!({"QueueName": "q"}),
    )
    .await["QueueUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    let started = tokio::time::Instant::now();
    let waiter = {
        let (sqs, url) = (sqs.clone(), url.clone());
        tokio::spawn(async move {
            let patterns = Patterns::new(&TABLES);
            run(
                &sqs,
                &patterns,
                Operation::ReceiveMessage,
                json!({"QueueUrl": url, "WaitTimeSeconds": 20}),
            )
            .await
        })
    };
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(!waiter.is_finished(), "still waiting after 0.6 s of 20");
    run(
        &sqs,
        &patterns,
        Operation::SendMessage,
        json!({"QueueUrl": url, "MessageBody": "hello"}),
    )
    .await;
    let got = waiter.await.unwrap();
    assert_eq!(got["Messages"][0]["Body"], "hello");
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "it did not wait out the 20 s: {:?}",
        started.elapsed()
    );
}

// Real time: the writer is a thread, and a paused clock would jump ahead while the service waits for it.
#[tokio::test]
async fn a_receive_with_nothing_to_find_waits_the_whole_time_then_answers_empty() {
    let (sqs, patterns) = (service(), Patterns::new(&TABLES));
    let url = run(
        &sqs,
        &patterns,
        Operation::CreateQueue,
        json!({"QueueName": "q"}),
    )
    .await["QueueUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    let started = tokio::time::Instant::now();
    let got = run(
        &sqs,
        &patterns,
        Operation::ReceiveMessage,
        json!({"QueueUrl": url, "WaitTimeSeconds": 2}),
    )
    .await;
    assert_eq!(got, json!({}));
    assert!(
        started.elapsed() >= Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
}

// Real time: the writer is a thread, and a paused clock would jump ahead while the service waits for it.
#[tokio::test]
async fn the_queues_default_wait_applies_when_the_request_names_none() {
    let (sqs, patterns) = (service(), Patterns::new(&TABLES));
    let created = run(
        &sqs,
        &patterns,
        Operation::CreateQueue,
        json!({"QueueName": "q", "Attributes": {"ReceiveMessageWaitTimeSeconds": "2"}}),
    )
    .await;
    let url = created["QueueUrl"].as_str().unwrap().to_owned();
    let started = tokio::time::Instant::now();
    run(
        &sqs,
        &patterns,
        Operation::ReceiveMessage,
        json!({"QueueUrl": url}),
    )
    .await;
    assert!(
        started.elapsed() >= Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    // Asking for no wait overrides the queue's.
    let started = tokio::time::Instant::now();
    run(
        &sqs,
        &patterns,
        Operation::ReceiveMessage,
        json!({"QueueUrl": url, "WaitTimeSeconds": 0}),
    )
    .await;
    assert!(started.elapsed() < Duration::from_secs(1));
}
