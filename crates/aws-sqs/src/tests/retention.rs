//! Retention without a receive: an expired message is counted out at once, removed by the next call to the service, and removed with
//! no call at all by a timer that runs only while some queue holds a message. And `DozePeek`, which reads without receiving.

use std::time::Duration;

use aws_wire::service::Service;
use doze_runtime::{Ctx, Service as _, Tasks};

use super::*;
use crate::Sqs;
use crate::core::{Ask, PEEK_OP};

fn depth(w: &mut World, url: &str) -> (String, String) {
    let a = w.ok(
        Operation::GetQueueAttributes,
        &json!({"QueueUrl": url, "AttributeNames": ["ApproximateNumberOfMessages", "ApproximateNumberOfMessagesNotVisible"]}),
    );
    (
        a["Attributes"]["ApproximateNumberOfMessages"]
            .as_str()
            .unwrap()
            .to_owned(),
        a["Attributes"]["ApproximateNumberOfMessagesNotVisible"]
            .as_str()
            .unwrap()
            .to_owned(),
    )
}

fn short_lived(w: &mut World, name: &str) -> String {
    w.ok(
        Operation::CreateQueue,
        &json!({"QueueName": name, "Attributes": {"MessageRetentionPeriod": "60"}}),
    )["QueueUrl"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn an_expired_message_is_not_counted_and_the_next_call_removes_it() {
    let mut w = World::new();
    let url = short_lived(&mut w, "brief");
    w.ok(
        Operation::SendMessage,
        &json!({"QueueUrl": url, "MessageBody": "a"}),
    );
    w.clock.advance(30_000);
    w.ok(
        Operation::SendMessage,
        &json!({"QueueUrl": url, "MessageBody": "b"}),
    );
    assert_eq!(depth(&mut w, &url), ("2".into(), "0".into()));
    assert_eq!(
        w.core.next_due(),
        Some(1_700_000_060_000),
        "the timer is due when the first message's retention passes"
    );

    w.clock.advance(31_000);
    assert_eq!(
        depth(&mut w, &url),
        ("1".into(), "0".into()),
        "the first is past its retention: gone, though nobody received"
    );
    assert_eq!(
        w.core.queues["brief"].messages.len(),
        1,
        "and that call removed it"
    );
    assert_eq!(w.core.next_due(), Some(1_700_000_090_000));

    // A call to another queue removes it as well: the check is the service's, not the queue's.
    w.queue("other");
    w.clock.advance(30_000);
    w.ok(Operation::ListQueues, &json!({}));
    assert!(w.core.queues["brief"].messages.is_empty());
    assert_eq!(w.core.next_due(), None, "nothing left: no timer");
}

#[test]
fn an_expired_message_in_flight_is_counted_out_too() {
    let mut w = World::new();
    let url = short_lived(&mut w, "brief");
    w.ok(
        Operation::SendMessage,
        &json!({"QueueUrl": url, "MessageBody": "a"}),
    );
    w.ok(
        Operation::ReceiveMessage,
        &json!({"QueueUrl": url, "VisibilityTimeout": 600}),
    );
    assert_eq!(depth(&mut w, &url), ("0".into(), "1".into()));
    w.clock.advance(60_000);
    assert_eq!(depth(&mut w, &url), ("0".into(), "0".into()));
}

#[tokio::test(flavor = "current_thread")]
async fn a_tick_removes_every_expired_message_and_says_nothing_is_due() {
    let mut w = World::new();
    let url = short_lived(&mut w, "brief");
    for body in ["a", "b", "c"] {
        w.ok(
            Operation::SendMessage,
            &json!({"QueueUrl": url, "MessageBody": body}),
        );
    }
    w.clock.advance(61_000);
    let answer = w.core.handle(Ask::Tick).await;
    assert!(answer.reply.is_ok());
    assert_eq!(answer.next_due, None);
    assert!(w.core.queues["brief"].messages.is_empty());
}

fn peek(w: &mut World, request: &Value) -> Result<Value, ApiError> {
    let mut call = call_of(&w.patterns, Operation::ReceiveMessage, request)?;
    call.op = PEEK_OP;
    let out = w.core.dispatch(&call)?;
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

    let ask = json!({"QueueUrl": url, "MaxNumberOfMessages": 50, "AttributeNames": ["All"], "MessageAttributeNames": ["All"]});
    let first = peek(&mut w, &ask).unwrap();
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
    assert_eq!(m["MessageAttributes"]["k"]["StringValue"], "b");
    assert!(
        m.get("ReceiptHandle").is_none(),
        "never received: no handle"
    );
    assert_eq!(
        peek(&mut w, &ask).unwrap(),
        first,
        "a second look sees the same"
    );
    assert_eq!(depth(&mut w, &url), ("2".into(), "1".into()));
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
    let got = peek(&mut w, &json!({"QueueUrl": url})).unwrap();
    assert_eq!(got["Messages"].as_array().unwrap().len(), 3);
    let one = peek(&mut w, &json!({"QueueUrl": url, "MaxNumberOfMessages": 1})).unwrap();
    assert_eq!(one["Messages"].as_array().unwrap().len(), 1);
    let e = peek(
        &mut w,
        &json!({"QueueUrl": url, "MaxNumberOfMessages": 101}),
    )
    .unwrap_err();
    assert_eq!(e.code, "InvalidParameterValue");
    let e = peek(
        &mut w,
        &json!({"QueueUrl": "http://localhost:4566/000000000000/none"}),
    )
    .unwrap_err();
    assert_eq!(e.shape.as_deref(), Some("QueueDoesNotExist"));
}

/// The timer runs while a queue holds a message, removes it when its retention passes with no call at all, and then stops.
#[tokio::test(start_paused = true)]
async fn the_retention_timer_runs_only_while_a_queue_holds_messages() {
    let clock = Arc::new(ManualClock::at(1_700_000_000_000));
    let tasks = Tasks::new();
    let ctx = Ctx {
        dir: Box::leak(Box::new(TempDir::new())).0.clone(),
        idle: Duration::from_secs(3600),
        tasks: tasks.clone(),
        clock: clock.clone(),
        ext: None,
    };
    let sqs = Sqs::new(Arc::new(ctx));
    let patterns = Patterns::new(&TABLES);
    let run = |op: Operation, request: Value| {
        let call = call_of(&patterns, op, &request).unwrap();
        let sqs = &sqs;
        async move { output_json(op, &sqs.call(call).await.unwrap()) }
    };
    let url = run(
        Operation::CreateQueue,
        json!({"QueueName": "q", "Attributes": {"MessageRetentionPeriod": "60"}}),
    )
    .await["QueueUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(!sqs.expiry_armed("us-east-1"), "an empty queue: no timer");
    run(
        Operation::SendMessage,
        json!({"QueueUrl": url, "MessageBody": "a"}),
    )
    .await;
    assert!(sqs.expiry_armed("us-east-1"));
    clock.advance(61_000);
    tokio::time::sleep(Duration::from_secs(61)).await;
    // The tick's write is the writer thread's, in real time: give it a moment.
    for _ in 0..2000 {
        if !sqs.expiry_armed("us-east-1") {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
        tokio::task::yield_now().await;
    }
    assert!(
        !sqs.expiry_armed("us-east-1"),
        "the message expired with no call, and with nothing left the timer stopped"
    );
    tasks.shutdown().await;
}
