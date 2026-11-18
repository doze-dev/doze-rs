//! The state machine without a socket: requests are decoded against the model and dispatched on a core with a manual clock,
//! so visibility and delay are exact.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::sync::Arc;

use aws_model_sqs::Operation;
use aws_model_sqs::tables::TABLES;
use aws_schema::arena::Arena;
use aws_schema::enc::encode;
use aws_schema::json::decode;
use aws_schema::patterns::Patterns;
use aws_wire::ApiError;
use aws_wire::RequestId;
use aws_wire::service::{Call, Caller};
use doze_runtime::ManualClock;
use serde_json::{Value, json};

use crate::core::Core;

struct World {
    core: Core,
    clock: Arc<ManualClock>,
    patterns: Patterns,
}

impl World {
    fn new() -> Self {
        let clock = Arc::new(ManualClock::at(1_700_000_000_000));
        let core = Core::new(clock.clone());
        Self {
            core,
            clock,
            patterns: Patterns::new(&TABLES),
        }
    }

    /// Runs one operation: the request as JSON in, the answer as JSON (or the error) out.
    fn run(&mut self, op: Operation, request: &Value) -> Result<Value, ApiError> {
        let call = call_of(&self.patterns, op, request)?;
        let out = self.core.dispatch(&call);
        // What the call changed is on disk (if there is a disk) before its answer is looked at.
        if let Some(saved) = self.core.flush().unwrap() {
            saved.wait().unwrap();
        }
        Ok(output_json(op, &out?))
    }

    fn ok(&mut self, op: Operation, request: &Value) -> Value {
        self.run(op, request)
            .unwrap_or_else(|e| panic!("{op:?}: {} {}", e.code, e.message))
    }

    fn err(&mut self, op: Operation, request: &Value) -> (u16, String, String) {
        let e = self.run(op, request).unwrap_err();
        (
            e.status,
            e.shape
                .map_or_else(|| e.code.to_string(), |s| s.to_string()),
            e.message,
        )
    }

    fn queue(&mut self, name: &str) -> String {
        self.ok(Operation::CreateQueue, &json!({"QueueName": name}))["QueueUrl"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

#[test]
fn a_queue_from_creation_to_deletion() {
    let mut w = World::new();
    let url = w.queue("jobs");
    assert_eq!(url, "http://localhost:4566/000000000000/jobs");
    assert_eq!(
        w.queue("jobs"),
        url,
        "creating it again with no attributes is the same queue"
    );
    assert_eq!(
        w.ok(Operation::GetQueueUrl, &json!({"QueueName": "jobs"}))["QueueUrl"],
        url
    );

    let names = [
        "VisibilityTimeout",
        "DelaySeconds",
        "MaximumMessageSize",
        "MessageRetentionPeriod",
        "ReceiveMessageWaitTimeSeconds",
        "QueueArn",
        "SqsManagedSseEnabled",
    ];
    let attrs = w.ok(
        Operation::GetQueueAttributes,
        &json!({"QueueUrl": url, "AttributeNames": names}),
    );
    assert_eq!(
        attrs["Attributes"],
        json!({"DelaySeconds": "0", "MaximumMessageSize": "1048576", "MessageRetentionPeriod": "345600", "QueueArn": "arn:aws:sqs:us-east-1:000000000000:jobs",
               "ReceiveMessageWaitTimeSeconds": "0", "SqsManagedSseEnabled": "true", "VisibilityTimeout": "30"})
    );

    let sent = w.ok(
        Operation::SendMessage,
        &json!({"QueueUrl": url, "MessageBody": "hello"}),
    );
    assert_eq!(sent["MD5OfMessageBody"], "5d41402abc4b2a76b9719d911017c592");
    let got = w.ok(
        Operation::ReceiveMessage,
        &json!({"QueueUrl": url, "MaxNumberOfMessages": 10}),
    );
    let m = &got["Messages"][0];
    assert_eq!(
        (m["Body"].as_str(), m["MessageId"].as_str()),
        (Some("hello"), sent["MessageId"].as_str())
    );
    w.ok(
        Operation::DeleteMessage,
        &json!({"QueueUrl": url, "ReceiptHandle": m["ReceiptHandle"]}),
    );
    assert_eq!(
        w.ok(Operation::ReceiveMessage, &json!({"QueueUrl": url})),
        json!({}),
        "nothing is left to receive"
    );
    w.ok(Operation::DeleteQueue, &json!({"QueueUrl": url}));
    assert_eq!(
        w.err(Operation::GetQueueUrl, &json!({"QueueName": "jobs"}))
            .1,
        "QueueDoesNotExist"
    );
}

#[test]
fn a_received_message_hides_until_its_visibility_timeout_and_counts_its_receives() {
    let mut w = World::new();
    let url = w.queue("q");
    w.ok(
        Operation::SendMessage,
        &json!({"QueueUrl": url, "MessageBody": "x"}),
    );
    let ask = json!({"QueueUrl": url, "VisibilityTimeout": 10, "MessageSystemAttributeNames": ["ApproximateReceiveCount"]});
    let first = w.ok(Operation::ReceiveMessage, &ask);
    assert_eq!(
        first["Messages"][0]["Attributes"]["ApproximateReceiveCount"],
        "1"
    );
    w.clock.advance(9_999);
    assert_eq!(
        w.ok(Operation::ReceiveMessage, &ask),
        json!({}),
        "still hidden one millisecond early"
    );
    w.clock.advance(1);
    let again = w.ok(Operation::ReceiveMessage, &ask);
    assert_eq!(
        again["Messages"][0]["Attributes"]["ApproximateReceiveCount"],
        "2"
    );
    assert_ne!(
        again["Messages"][0]["ReceiptHandle"],
        first["Messages"][0]["ReceiptHandle"]
    );
}

#[test]
fn a_delayed_message_waits_and_the_counters_say_where_messages_are() {
    let mut w = World::new();
    let url = w.queue("q");
    w.ok(
        Operation::SendMessage,
        &json!({"QueueUrl": url, "MessageBody": "later", "DelaySeconds": 5}),
    );
    w.ok(
        Operation::SendMessage,
        &json!({"QueueUrl": url, "MessageBody": "now"}),
    );
    let counts = |w: &mut World| {
        let names = [
            "ApproximateNumberOfMessages",
            "ApproximateNumberOfMessagesNotVisible",
            "ApproximateNumberOfMessagesDelayed",
        ];
        let a = w.ok(
            Operation::GetQueueAttributes,
            &json!({"QueueUrl": url, "AttributeNames": names}),
        );
        names.map(|n| a["Attributes"][n].as_str().unwrap().to_owned())
    };
    assert_eq!(counts(&mut w), ["1", "0", "1"]);
    let got = w.ok(
        Operation::ReceiveMessage,
        &json!({"QueueUrl": url, "MaxNumberOfMessages": 10}),
    );
    assert_eq!(got["Messages"].as_array().unwrap().len(), 1);
    assert_eq!(counts(&mut w), ["0", "1", "1"]);
    w.clock.advance(5_000);
    let later = w.ok(Operation::ReceiveMessage, &json!({"QueueUrl": url}));
    assert_eq!(later["Messages"][0]["Body"], "later");
}

#[test]
fn changing_visibility_moves_the_deadline() {
    let mut w = World::new();
    let url = w.queue("q");
    w.ok(
        Operation::SendMessage,
        &json!({"QueueUrl": url, "MessageBody": "x"}),
    );
    let handle =
        w.ok(Operation::ReceiveMessage, &json!({"QueueUrl": url}))["Messages"][0]["ReceiptHandle"]
            .clone();
    w.ok(
        Operation::ChangeMessageVisibility,
        &json!({"QueueUrl": url, "ReceiptHandle": handle, "VisibilityTimeout": 0}),
    );
    assert_eq!(
        w.ok(Operation::ReceiveMessage, &json!({"QueueUrl": url}))["Messages"][0]["Body"],
        "x",
        "visible at once"
    );
}

#[test]
fn the_refusals_aws_recorded() {
    let mut w = World::new();
    let url = w.queue("q");
    let e = |w: &mut World, op, req: Value| w.err(op, &req);
    assert_eq!(
        e(
            &mut w,
            Operation::GetQueueUrl,
            json!({"QueueName": "absent"})
        ),
        (
            400,
            "QueueDoesNotExist".into(),
            "The specified queue does not exist.".into()
        )
    );
    let (status, shape, message) = e(
        &mut w,
        Operation::CreateQueue,
        json!({"QueueName": "q", "Attributes": {"VisibilityTimeout": "77"}}),
    );
    assert_eq!((status, shape.as_str()), (400, "QueueNameExists"));
    assert_eq!(
        message,
        "A queue already exists with the same name and a different value for attribute VisibilityTimeout"
    );
    assert_eq!(
        e(
            &mut w,
            Operation::DeleteMessage,
            json!({"QueueUrl": url, "ReceiptHandle": "not-a-receipt-handle"})
        ),
        (
            400,
            "ReceiptHandleIsInvalid".into(),
            "The input receipt handle is invalid.".into()
        )
    );
    assert_eq!(
        e(
            &mut w,
            Operation::SetQueueAttributes,
            json!({"QueueUrl": url, "Attributes": {"VisibilityTimeout": "43201"}})
        ),
        (
            400,
            "InvalidAttributeValue".into(),
            "Invalid value for the parameter VisibilityTimeout.".into()
        )
    );
    assert_eq!(
        e(
            &mut w,
            Operation::SetQueueAttributes,
            json!({"QueueUrl": url, "Attributes": {"QueueArn": "x"}})
        ),
        (
            400,
            "InvalidAttributeName".into(),
            "Unknown Attribute QueueArn.".into()
        )
    );
}

#[test]
fn a_receive_out_of_range_is_refused_with_the_reason() {
    let mut w = World::new();
    let url = w.queue("q");
    for (key, value, parameter, reason) in [
        (
            "MaxNumberOfMessages",
            0,
            "MaxNumberOfMessages",
            "Must be between 1 and 10, if provided.",
        ),
        (
            "MaxNumberOfMessages",
            11,
            "MaxNumberOfMessages",
            "Must be between 1 and 10, if provided.",
        ),
        (
            "WaitTimeSeconds",
            21,
            "WaitTimeSeconds",
            "Must be >= 0 and <= 20, if provided.",
        ),
        (
            "VisibilityTimeout",
            43201,
            "VisibilityTimeout",
            "Must be between 0 and 43200, if provided.",
        ),
    ] {
        let got = w.err(
            Operation::ReceiveMessage,
            &json!({"QueueUrl": url, key: value}),
        );
        assert_eq!(
            got,
            (
                400,
                "InvalidParameterValue".into(),
                format!("Value {value} for parameter {parameter} is invalid. Reason: {reason}")
            )
        );
    }
}

#[test]
fn queues_list_in_name_order_with_a_prefix_and_pages() {
    let mut w = World::new();
    for n in ["b-2", "a-1", "b-1", "c"] {
        w.queue(n);
    }
    let all = w.ok(Operation::ListQueues, &json!({}));
    let tail = |v: &Value| {
        v["QueueUrls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u.as_str().unwrap().rsplit('/').next().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(tail(&all), ["a-1", "b-1", "b-2", "c"]);
    assert_eq!(
        tail(&w.ok(Operation::ListQueues, &json!({"QueueNamePrefix": "b-"}))),
        ["b-1", "b-2"]
    );
    let page = w.ok(Operation::ListQueues, &json!({"MaxResults": 3}));
    assert_eq!(tail(&page), ["a-1", "b-1", "b-2"]);
    let rest = w.ok(
        Operation::ListQueues,
        &json!({"MaxResults": 3, "NextToken": page["NextToken"]}),
    );
    assert_eq!(
        (tail(&rest), rest.get("NextToken")),
        (vec!["c".to_owned()], None)
    );
}

/// A request decoded against the model, as the gateway would hand it to a service.
fn call_of(patterns: &Patterns, op: Operation, request: &Value) -> Result<Call, ApiError> {
    let body = request.to_string();
    let shape = TABLES.op(op.index()).unwrap().input.unwrap();
    let mut arena = Arena::new();
    let root = decode(&TABLES, patterns, shape, &body, &mut arena)
        .map_err(|v| crate::Sqs::invalid_for_tests(&v))?;
    let caller = Caller {
        access_key_id: "test".into(),
        account_id: "000000000000".into(),
        region: "us-east-1".into(),
        endpoint: "http://localhost:4566".into(),
        service: None,
        session_token: None,
    };
    Ok(Call {
        op: op.index(),
        id: RequestId::new(),
        caller,
        body,
        arena,
        root,
    })
}

/// An answer as JSON.
fn output_json(op: Operation, out: &aws_wire::service::Output) -> Value {
    let Some(shape) = TABLES.op(op.index()).unwrap().output else {
        return json!({});
    };
    if out.root == aws_schema::arena::NIL {
        return json!({});
    }
    let mut bytes = Vec::new();
    encode(&TABLES, shape, &out.arena, "", out.root, &mut bytes).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

mod long_poll;
mod persistence;
mod retention;

/// A directory of its own that goes away with it.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("doze-aws-sqs-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}
