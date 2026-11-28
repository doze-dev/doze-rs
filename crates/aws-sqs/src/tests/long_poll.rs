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
        ext: None,
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

async fn queue_url(sqs: &Sqs, patterns: &Patterns, name: &str, attrs: Value) -> String {
    run(
        sqs,
        patterns,
        Operation::CreateQueue,
        json!({"QueueName": name, "Attributes": attrs}),
    )
    .await["QueueUrl"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// A watch on a queue is woken by what can give it a message, and by nothing else: not a read of it, not a send to another queue.
/// Once each was (one wake-up for the whole service, fired by every call that was not a receive), and a mapping that read the
/// queue's attributes woke itself: one core busy forever.
// Real time: the writer is a thread.
#[tokio::test]
async fn a_watch_on_a_queue_is_woken_by_an_arrival_there_and_nothing_else() {
    let (sqs, patterns) = (service(), Patterns::new(&TABLES));
    let url = queue_url(&sqs, &patterns, "q", json!({})).await;
    let other = queue_url(&sqs, &patterns, "other", json!({})).await;
    let watch = sqs.watch("arn:aws:sqs:us-east-1:000000000000:q").unwrap();
    woken_only_by_a_send(&sqs, &patterns, &watch, &url, &other).await;
    assert_eq!(sqs.arrivals.watched(), 1);
    drop(watch);
    assert_eq!(
        sqs.arrivals.watched(),
        0,
        "a queue nobody waits on costs nothing"
    );
}

async fn woken_only_by_a_send(
    sqs: &Sqs,
    patterns: &Patterns,
    watch: &crate::Watch,
    url: &str,
    other: &str,
) {
    let quiet = watch.notified();
    tokio::pin!(quiet);
    quiet.as_mut().enable();
    for (op, body) in [
        (
            Operation::SendMessage,
            json!({"QueueUrl": other, "MessageBody": "x"}),
        ),
        (
            Operation::GetQueueAttributes,
            json!({"QueueUrl": url, "AttributeNames": ["All"]}),
        ),
        (Operation::ReceiveMessage, json!({"QueueUrl": url})),
        (Operation::PurgeQueue, json!({"QueueUrl": url})),
    ] {
        run(sqs, patterns, op, body).await;
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(50), quiet.as_mut())
            .await
            .is_err(),
        "nothing arrived in q"
    );
    run(
        sqs,
        patterns,
        Operation::SendMessage,
        json!({"QueueUrl": url, "MessageBody": "x"}),
    )
    .await;
    tokio::time::timeout(Duration::from_secs(1), quiet)
        .await
        .expect("a send to q wakes it");
}

/// A long poll on a queue whose only message is in flight sleeps until the message shows, and is answered then: it does not look
/// again every quarter second, and it does not wait out its 20 seconds.
// Real time: the writer is a thread.
#[tokio::test]
async fn a_long_poll_is_answered_when_an_in_flight_message_shows() {
    let (sqs, patterns) = (service(), Patterns::new(&TABLES));
    let url = queue_url(&sqs, &patterns, "q", json!({"VisibilityTimeout": "1"})).await;
    run(
        &sqs,
        &patterns,
        Operation::SendMessage,
        json!({"QueueUrl": url, "MessageBody": "x"}),
    )
    .await;
    let first = run(
        &sqs,
        &patterns,
        Operation::ReceiveMessage,
        json!({"QueueUrl": url}),
    )
    .await;
    assert_eq!(first["Messages"].as_array().map(Vec::len), Some(1));
    let started = tokio::time::Instant::now();
    let again = run(
        &sqs,
        &patterns,
        Operation::ReceiveMessage,
        json!({"QueueUrl": url, "WaitTimeSeconds": 20}),
    )
    .await;
    assert_eq!(again["Messages"][0]["Body"], "x");
    let took = started.elapsed();
    assert!(
        took >= Duration::from_millis(500) && took < Duration::from_secs(3),
        "{took:?}"
    );
}
