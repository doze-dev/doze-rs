//! What an idle mapping costs: the calls its poller makes while there is nothing to read.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use aws_schema::meta::ServiceMeta;
use aws_schema::tables::Tables;
use aws_wire::service::{BoxFuture, Call, Reply, Service};
use serde_json::json;

use crate::tests::{ACCOUNT, World};

/// SQS on the bus, counting the calls made to it.
struct Counting {
    sqs: Arc<aws_sqs::Sqs>,
    calls: AtomicUsize,
}

impl Service for Counting {
    fn meta(&self) -> &'static ServiceMeta {
        self.sqs.meta()
    }

    fn tables(&self) -> &'static Tables {
        self.sqs.tables()
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Reply> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.sqs.call(call)
    }
}

/// An enabled mapping on an empty queue makes its first read and then waits: another queue's traffic, and reads of its own queue,
/// do not wake it (once each did, and the poller's own `GetQueueAttributes` woke it again: a core at 73% forever).
#[tokio::test]
async fn an_idle_mapping_on_an_empty_queue_makes_no_calls() {
    let w = World::new();
    let sqs = Arc::clone(&w.lambda.inner.sources.get().expect("connected").sqs);
    let counting = Arc::new(Counting {
        sqs,
        calls: AtomicUsize::new(0),
    });
    w.peers.register(counting.clone());
    w.create(
        "p",
        "python3.12",
        "index.handler",
        &[("index.py", b"", 0o644)],
    )
    .await;
    let made = |name: &'static str| w.peer("sqs", "CreateQueue", json!({"QueueName": name}));
    made("q").await;
    let other = made("other").await["QueueUrl"].as_str().unwrap().to_owned();
    let arn = format!("arn:aws:sqs:us-east-1:{ACCOUNT}:q");
    w.ok(
        "CreateEventSourceMapping",
        json!({"FunctionName": "p", "EventSourceArn": arn, "BatchSize": 10}),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let settled = counting.calls.load(Ordering::Relaxed);
    // Traffic on another queue: sends, and reads of what it holds.
    for _ in 0..20 {
        w.peer(
            "sqs",
            "SendMessage",
            json!({"QueueUrl": other, "MessageBody": "x"}),
        )
        .await;
        w.peer(
            "sqs",
            "GetQueueAttributes",
            json!({"QueueUrl": other, "AttributeNames": ["All"]}),
        )
        .await;
    }
    let ours = 40;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let made = counting.calls.load(Ordering::Relaxed) - settled - ours;
    assert_eq!(
        made, 0,
        "the poller made {made} calls while its queue was empty"
    );
    assert!(settled < 20, "starting took {settled} calls");
    w.stop().await;
}
