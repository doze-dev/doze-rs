//! The local look at a queue (`/_doze/sqs/<region>/<queue>/peek`): it reads without receiving, and is not on SQS's endpoint.

use std::time::Duration;

use aws_wire::service::{Service, WebRequest};
use doze_runtime::{Ctx, Tasks};

use super::*;
use crate::Sqs;

fn depth(w: &mut World, url: &str) -> (Value, Value) {
    let a = w.ok(
        Operation::GetQueueAttributes,
        &json!({"QueueUrl": url, "AttributeNames": ["ApproximateNumberOfMessages", "ApproximateNumberOfMessagesNotVisible"]}),
    );
    (
        a["Attributes"]["ApproximateNumberOfMessages"].clone(),
        a["Attributes"]["ApproximateNumberOfMessagesNotVisible"].clone(),
    )
}

fn peek(w: &World, name: &str, max: usize) -> Result<Value, ApiError> {
    let out = w.core.peek(name, max)?;
    Ok(output_json(Operation::ReceiveMessage, &out))
}

#[test]
fn a_peek_reads_every_visible_message_and_changes_nothing() {
    let mut w = World::new();
    let url = w.queue("jobs");
    for body in ["a", "b", "c"] {
        w.ok(
            Operation::SendMessage,
            &json!({"QueueUrl": url, "MessageBody": body, "MessageAttributes": {"k": {"DataType": "String", "StringValue": body}}}),
        );
    }
    w.ok(
        Operation::SendMessage,
        &json!({"QueueUrl": url, "MessageBody": "later", "DelaySeconds": 60}),
    );
    let got = w.ok(Operation::ReceiveMessage, &json!({"QueueUrl": url}));
    assert_eq!(got["Messages"][0]["Body"], "a");

    let first = peek(&w, "jobs", 50).unwrap();
    let bodies: Vec<&str> = first["Messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["Body"].as_str().unwrap())
        .collect();
    assert_eq!(
        bodies,
        ["b", "c"],
        "not the one in flight, not the delayed one"
    );
    let m = &first["Messages"][0];
    assert_eq!(m["Attributes"]["ApproximateReceiveCount"], "0");
    assert!(m["Attributes"].get("SentTimestamp").is_some());
    assert!(
        m["Attributes"].get("SenderId").is_none(),
        "a look has no sender"
    );
    assert_eq!(m["MessageAttributes"]["k"]["StringValue"], "b");
    assert!(
        m.get("ReceiptHandle").is_none(),
        "never received: no handle"
    );
    assert_eq!(
        peek(&w, "jobs", 50).unwrap(),
        first,
        "a second look sees the same"
    );
    assert_eq!(depth(&mut w, &url), (json!("2"), json!("1")));
    assert_eq!(
        w.ok(
            Operation::ReceiveMessage,
            &json!({"QueueUrl": url, "AttributeNames": ["ApproximateReceiveCount"]})
        )["Messages"][0]["Attributes"]["ApproximateReceiveCount"],
        "1",
        "the peeks were not receives"
    );
}

#[test]
fn a_peek_shows_every_group_of_a_fifo_queue_and_is_bounded() {
    let mut w = World::new();
    let url = w.ok(
        Operation::CreateQueue,
        &json!({"QueueName": "jobs.fifo", "Attributes": {"FifoQueue": "true", "ContentBasedDeduplication": "true"}}),
    )["QueueUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    for (i, body) in ["a", "b", "c"].iter().enumerate() {
        w.ok(
            Operation::SendMessage,
            &json!({"QueueUrl": url, "MessageBody": body, "MessageGroupId": format!("g{}", i % 2)}),
        );
    }
    let got = peek(&w, "jobs.fifo", 10).unwrap();
    assert_eq!(got["Messages"].as_array().unwrap().len(), 3);
    assert_eq!(got["Messages"][0]["Attributes"]["MessageGroupId"], "g0");
    let one = peek(&w, "jobs.fifo", 1).unwrap();
    assert_eq!(one["Messages"].as_array().unwrap().len(), 1);
    let e = peek(&w, "none", 10).unwrap_err();
    assert_eq!(e.shape.as_deref(), Some("QueueDoesNotExist"));
}

fn get(path: &str, query: &str) -> WebRequest {
    WebRequest {
        method: "GET".into(),
        host: "localhost:4566".into(),
        path: path.into(),
        query: query.into(),
        headers: Vec::new(),
        body: Vec::new().into(),
        endpoint: "http://localhost:4566".into(),
        signature: aws_wire::service::WebSignature::Unsigned,
    }
}

fn body(r: &aws_wire::service::WebReply) -> Value {
    serde_json::from_slice(&r.body).unwrap()
}

/// Through the service's web side, as the gateway hands it a request.
#[tokio::test]
async fn the_hook_answers_at_its_own_address_only() {
    let tasks = Tasks::new();
    let dir = TempDir::new();
    let ctx = Ctx {
        dir: dir.0.clone(),
        idle: Duration::from_secs(3600),
        tasks: tasks.clone(),
        clock: Arc::new(ManualClock::at(1_700_000_000_000)),
        ext: None,
    };
    let sqs = Sqs::new(Arc::new(ctx));
    let web = sqs.web().unwrap();
    assert!(web.owns("localhost", "/_doze/sqs/us-east-1/q/peek"));
    assert!(!web.owns("localhost", "/"));
    assert!(web.local(), "the gateway guards it");
    // No region has anything yet: nothing is woken, and the queue does not exist.
    let none = web.serve(get("/_doze/sqs/eu-west-1/q/peek", "")).await;
    assert_eq!(
        (none.status, body(&none)["Code"].clone()),
        (400, json!("QueueDoesNotExist"))
    );

    let patterns = Patterns::new(&TABLES);
    let mut call = call_of(
        &patterns,
        Operation::CreateQueue,
        &json!({"QueueName": "q"}),
    )
    .unwrap();
    let url = output_json(Operation::CreateQueue, &sqs.call(call).await.unwrap())["QueueUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    for b in ["one", "two"] {
        call = call_of(
            &patterns,
            Operation::SendMessage,
            &json!({"QueueUrl": url, "MessageBody": b}),
        )
        .unwrap();
        sqs.call(call).await.unwrap();
    }
    let seen = web.serve(get("/_doze/sqs/us-east-1/q/peek", "max=1")).await;
    assert_eq!(seen.status, 200);
    assert_eq!(body(&seen)["Messages"][0]["Body"], "one");
    assert_eq!(body(&seen)["Messages"].as_array().map(Vec::len), Some(1));
    let all = web.serve(get("/_doze/sqs/us-east-1/q/peek", "")).await;
    assert_eq!(body(&all)["Messages"].as_array().map(Vec::len), Some(2));

    let bad = web
        .serve(get("/_doze/sqs/us-east-1/q/peek", "max=101"))
        .await;
    assert_eq!(
        (bad.status, body(&bad)["Code"].clone()),
        (400, json!("InvalidParameterValue"))
    );
    let elsewhere = web.serve(get("/_doze/sqs/us-east-1/q", "")).await;
    assert_eq!(elsewhere.status, 404);
    let mut post = get("/_doze/sqs/us-east-1/q/peek", "");
    post.method = "POST".into();
    assert_eq!(web.serve(post).await.status, 405);
    tasks.shutdown().await;
}
