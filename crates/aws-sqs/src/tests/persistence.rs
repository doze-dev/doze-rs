//! State on disk: after every call of a broad scenario the service is stopped and started again, and what it loads is exactly
//! what it had. A change that is made and not written shows up here as a difference.

use super::*;
use crate::persist::Persist;

const ACCOUNT: &str = "000000000000";

impl World {
    /// A world whose state lives in `dir`.
    fn on_disk(dir: &std::path::Path) -> Self {
        let mut w = Self::new();
        w.core = Core::restore(w.clock.clone(), Some(Persist::open(dir).unwrap()));
        w
    }

    /// Stops the service and starts it again on the same files.
    fn restart(&mut self, dir: &std::path::Path) {
        let old = std::mem::replace(&mut self.core, Core::new(self.clock.clone()));
        old.persist.unwrap().close();
        self.core = Core::restore(self.clock.clone(), Some(Persist::open(dir).unwrap()));
    }

    /// Everything the service holds that is meant to last.
    fn state(&self) -> String {
        format!(
            "{:?}\n{:?}\n{:?}",
            self.core.queues, self.core.deleted, self.core.tasks
        )
    }

    /// Runs `op`, then proves a restart changes nothing.
    fn run_and_restart(&mut self, dir: &std::path::Path, op: Operation, request: &Value) -> Value {
        let answer = self.ok(op, request);
        let before = self.state();
        self.restart(dir);
        assert_eq!(before, self.state(), "after {op:?} {request}");
        answer
    }
}

fn arn(name: &str) -> String {
    format!("arn:aws:sqs:us-east-1:{ACCOUNT}:{name}")
}

/// A world on disk that is stopped and started again after every call.
struct Run<'a> {
    w: World,
    dir: &'a std::path::Path,
}

impl Run<'_> {
    fn go(&mut self, op: Operation, request: Value) -> Value {
        self.w.run_and_restart(self.dir, op, &request)
    }

    /// Creates a queue and returns its URL.
    fn create(&mut self, request: Value) -> String {
        self.go(Operation::CreateQueue, request)["QueueUrl"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

/// A standard queue with settings and tags, messages in every state, and the edits that follow.
fn a_standard_queue(r: &mut Run<'_>) -> String {
    let url = r.create(json!({"QueueName": "jobs", "Attributes": {"VisibilityTimeout": "20", "DelaySeconds": "0"}, "tags": {"env": "dev"}}));
    for body in ["one", "two", "three"] {
        r.go(
            Operation::SendMessage,
            json!({"QueueUrl": url, "MessageBody": body, "MessageAttributes": {"kind": {"DataType": "String", "StringValue": body}}}),
        );
    }
    r.go(
        Operation::SendMessage,
        json!({"QueueUrl": url, "MessageBody": "later", "DelaySeconds": 30}),
    );
    // A received message is in flight with a handle, and a restart keeps it there.
    let got = r.go(
        Operation::ReceiveMessage,
        json!({"QueueUrl": url, "MaxNumberOfMessages": 2}),
    );
    let handle = got["Messages"][0]["ReceiptHandle"]
        .as_str()
        .unwrap()
        .to_owned();
    r.go(
        Operation::ChangeMessageVisibility,
        json!({"QueueUrl": url, "ReceiptHandle": handle, "VisibilityTimeout": 100}),
    );
    r.w.clock.advance(5_000);
    r.go(
        Operation::DeleteMessage,
        json!({"QueueUrl": url, "ReceiptHandle": handle}),
    );
    r.go(
        Operation::SetQueueAttributes,
        json!({"QueueUrl": url, "Attributes": {"MessageRetentionPeriod": "3600"}}),
    );
    r.go(
        Operation::TagQueue,
        json!({"QueueUrl": url, "Tags": {"team": "core"}}),
    );
    r.go(
        Operation::UntagQueue,
        json!({"QueueUrl": url, "TagKeys": ["env"]}),
    );
    r.go(
        Operation::AddPermission,
        json!({"QueueUrl": url, "Label": "l", "AWSAccountIds": [ACCOUNT], "Actions": ["SendMessage"]}),
    );
    url
}

/// A FIFO queue: sequence numbers, the deduplication window, a receive attempt. Returns its URL.
fn a_fifo_queue(r: &mut Run<'_>) -> String {
    let fifo = r.create(json!({"QueueName": "f.fifo", "Attributes": {"FifoQueue": "true", "ContentBasedDeduplication": "true"}}));
    for body in ["a", "b", "a"] {
        r.go(
            Operation::SendMessage,
            json!({"QueueUrl": fifo, "MessageBody": body, "MessageGroupId": "g"}),
        );
    }
    r.go(
        Operation::ReceiveMessage,
        json!({"QueueUrl": fifo, "ReceiveRequestAttemptId": "try-1"}),
    );
    fifo
}

/// A dead-letter queue, a message that is received too often, and a task that moves it back.
fn dead_letters(r: &mut Run<'_>) {
    let dlq = r.create(json!({"QueueName": "dead"}));
    let policy = json!({"deadLetterTargetArn": arn("dead"), "maxReceiveCount": 1}).to_string();
    let main = r.create(
        json!({"QueueName": "main", "Attributes": {"VisibilityTimeout": "1", "RedrivePolicy": policy}}),
    );
    r.go(
        Operation::SendMessage,
        json!({"QueueUrl": main, "MessageBody": "poison"}),
    );
    r.go(Operation::ReceiveMessage, json!({"QueueUrl": main}));
    r.w.clock.advance(2_000);
    r.go(Operation::ReceiveMessage, json!({"QueueUrl": main}));
    let moved = r.w.ok(Operation::ReceiveMessage, &json!({"QueueUrl": dlq}));
    assert_eq!(
        moved["Messages"][0]["Body"], "poison",
        "it reached the dead-letter queue"
    );
    r.go(
        Operation::StartMessageMoveTask,
        json!({"SourceArn": arn("dead")}),
    );
    r.w.clock.advance(1_000);
    r.go(
        Operation::ListMessageMoveTasks,
        json!({"SourceArn": arn("dead")}),
    );
}

#[test]
fn a_restart_changes_nothing_whatever_was_done_before_it() {
    let disk = TempDir::new();
    let mut r = Run {
        w: World::on_disk(&disk.0),
        dir: &disk.0,
    };
    let jobs = a_standard_queue(&mut r);
    let fifo = a_fifo_queue(&mut r);
    dead_letters(&mut r);
    // Purge, delete, and the name held for a minute after.
    r.go(Operation::PurgeQueue, json!({"QueueUrl": jobs}));
    r.go(Operation::DeleteQueue, json!({"QueueUrl": fifo}));
    let again = json!({"QueueName": "f.fifo", "Attributes": {"FifoQueue": "true"}});
    assert_eq!(
        r.w.err(Operation::CreateQueue, &again).1,
        "QueueDeletedRecently",
        "still held after a restart"
    );
    r.w.clock.advance(61_000);
    r.go(Operation::CreateQueue, again);
}

#[test]
fn messages_in_flight_stay_in_flight_across_a_restart_and_come_back_on_time() {
    let disk = TempDir::new();
    let dir = disk.0.as_path();
    let mut w = World::on_disk(dir);
    let url = w.queue("q");
    w.ok(
        Operation::SendMessage,
        &json!({"QueueUrl": url, "MessageBody": "x"}),
    );
    let got = w.ok(
        Operation::ReceiveMessage,
        &json!({"QueueUrl": url, "VisibilityTimeout": 10}),
    );
    let handle = got["Messages"][0]["ReceiptHandle"]
        .as_str()
        .unwrap()
        .to_owned();
    w.restart(dir);
    assert_eq!(
        w.ok(Operation::ReceiveMessage, &json!({"QueueUrl": url})),
        json!({}),
        "hidden: the restart did not free it"
    );
    w.clock.advance(10_000);
    let again = w.ok(
        Operation::ReceiveMessage,
        &json!({"QueueUrl": url, "MessageSystemAttributeNames": ["ApproximateReceiveCount"]}),
    );
    assert_eq!(
        again["Messages"][0]["Attributes"]["ApproximateReceiveCount"],
        "2"
    );
    // The handle from before the restart still deletes it.
    w.ok(
        Operation::DeleteMessage,
        &json!({"QueueUrl": url, "ReceiptHandle": handle}),
    );
    w.restart(dir);
    w.clock.advance(100_000);
    assert_eq!(
        w.ok(Operation::ReceiveMessage, &json!({"QueueUrl": url})),
        json!({})
    );
}

#[test]
fn a_call_writes_the_rows_it_changed_and_not_the_queue() {
    let mut w = World::new();
    let url = w.queue("big");
    for i in 0..300 {
        w.ok(
            Operation::SendMessage,
            &json!({"QueueUrl": url, "MessageBody": format!("message {i}")}),
        );
    }
    let rows = |w: &mut World, op: Operation, request: Value| {
        let call = call_of(&w.patterns, op, &request).unwrap();
        w.core.dispatch(&call).unwrap();
        w.core.stage().unwrap().rows()
    };
    assert_eq!(
        rows(
            &mut w,
            Operation::SendMessage,
            json!({"QueueUrl": url, "MessageBody": "one more"})
        ),
        1,
        "a send writes its message"
    );
    let got = rows(
        &mut w,
        Operation::ReceiveMessage,
        json!({"QueueUrl": url, "MaxNumberOfMessages": 3}),
    );
    assert_eq!(got, 3, "a receive writes the three messages it took");
    assert_eq!(
        rows(
            &mut w,
            Operation::GetQueueAttributes,
            json!({"QueueUrl": url})
        ),
        0,
        "a read writes nothing"
    );
}
