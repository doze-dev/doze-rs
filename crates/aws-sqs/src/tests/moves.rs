//! Message move tasks report what they moved.

use super::*;

fn arn(w: &mut World, url: &str) -> String {
    w.ok(
        Operation::GetQueueAttributes,
        &json!({"QueueUrl": url, "AttributeNames": ["QueueArn"]}),
    )["Attributes"]["QueueArn"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn a_finished_task_counts_what_it_moved() {
    let mut w = World::new();
    let dlq = w.queue("dlq");
    let dlq_arn = arn(&mut w, &dlq);
    let src = w.ok(
        Operation::CreateQueue,
        &json!({"QueueName": "src", "Attributes": {"RedrivePolicy": format!("{{\"deadLetterTargetArn\":\"{dlq_arn}\",\"maxReceiveCount\":\"1\"}}")}}),
    )["QueueUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    let src_arn = arn(&mut w, &src);
    w.ok(
        Operation::SendMessage,
        &json!({"QueueUrl": dlq, "MessageBody": "poison"}),
    );
    w.ok(
        Operation::StartMessageMoveTask,
        &json!({"SourceArn": dlq_arn, "DestinationArn": src_arn}),
    );
    let listed = w.ok(
        Operation::ListMessageMoveTasks,
        &json!({"SourceArn": dlq_arn}),
    );
    let task = &listed["Results"][0];
    // Recorded 2026-11-15: 1 moved, at once. (The 2026-11-09 recording showed 0: AWS's counter catching up.)
    assert_eq!(
        (
            task["Status"].as_str(),
            task["ApproximateNumberOfMessagesMoved"].as_i64()
        ),
        (Some("COMPLETED"), Some(1))
    );
}

/// A source queue whose dead-letter queue is `dlq`, and the dead-letter queue's ARN.
fn dead_letters(w: &mut World, dlq: &str) -> (String, String) {
    let dlq_arn = arn(w, dlq);
    let src = w.ok(
        Operation::CreateQueue,
        &json!({"QueueName": "src", "Attributes": {"RedrivePolicy": format!("{{\"deadLetterTargetArn\":\"{dlq_arn}\",\"maxReceiveCount\":\"1\"}}")}}),
    )["QueueUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    (src, dlq_arn)
}

fn last_task(w: &mut World, dlq_arn: &str) -> serde_json::Value {
    w.ok(
        Operation::ListMessageMoveTasks,
        &json!({"SourceArn": dlq_arn}),
    )["Results"][0]
        .clone()
}

/// Model `ListMessageMoveTasksResultEntry.FailureReason`: "only included if the task status is FAILED"; the value is the one
/// the SQS Developer Guide names for a message that was sent straight to the dead-letter queue.
#[test]
fn a_message_sent_straight_to_the_dead_letter_queue_fails_the_task() {
    let mut w = World::new();
    let dlq = w.queue("dlq");
    let (_src, dlq_arn) = dead_letters(&mut w, &dlq);
    w.ok(
        Operation::SendMessage,
        &json!({"QueueUrl": dlq, "MessageBody": "sent by hand"}),
    );
    w.ok(
        Operation::StartMessageMoveTask,
        &json!({"SourceArn": dlq_arn}),
    );
    let task = last_task(&mut w, &dlq_arn);
    assert_eq!(task["Status"], "FAILED");
    assert_eq!(task["FailureReason"], "CouldNotDetermineMessageSource");
    assert_eq!(task["ApproximateNumberOfMessagesMoved"], 0);
    assert_eq!(task["ApproximateNumberOfMessagesToMove"], 1);
    assert!(
        task.get("TaskHandle").is_none(),
        "only a running task has one"
    );
    // The message is still where it was, and a failed task no longer blocks the next.
    let attrs = w.ok(
        Operation::GetQueueAttributes,
        &json!({"QueueUrl": dlq, "AttributeNames": ["ApproximateNumberOfMessages"]}),
    );
    assert_eq!(attrs["Attributes"]["ApproximateNumberOfMessages"], "1");
    w.ok(
        Operation::StartMessageMoveTask,
        &json!({"SourceArn": dlq_arn}),
    );
}

#[test]
fn a_task_with_a_destination_does_not_need_a_source_and_other_states_name_no_reason() {
    let mut w = World::new();
    let dlq = w.queue("dlq");
    let (src, dlq_arn) = dead_letters(&mut w, &dlq);
    let src_arn = arn(&mut w, &src);
    w.ok(
        Operation::SendMessage,
        &json!({"QueueUrl": dlq, "MessageBody": "sent by hand"}),
    );
    w.ok(
        Operation::StartMessageMoveTask,
        &json!({"SourceArn": dlq_arn, "DestinationArn": src_arn}),
    );
    let task = last_task(&mut w, &dlq_arn);
    assert_eq!(task["Status"], "COMPLETED");
    assert!(task.get("FailureReason").is_none());
    assert_eq!(task["ApproximateNumberOfMessagesMoved"], 1);
}

#[test]
fn messages_before_the_sourceless_one_still_move() {
    let mut w = World::new();
    let dlq = w.queue("dlq");
    let (src, dlq_arn) = dead_letters(&mut w, &dlq);
    // One dead-lettered the real way (it carries its source), then one sent by hand.
    w.ok(
        Operation::SendMessage,
        &json!({"QueueUrl": src, "MessageBody": "poison"}),
    );
    w.ok(Operation::ReceiveMessage, &json!({"QueueUrl": src}));
    w.clock.advance(31_000);
    w.ok(Operation::ReceiveMessage, &json!({"QueueUrl": src}));
    w.ok(
        Operation::SendMessage,
        &json!({"QueueUrl": dlq, "MessageBody": "by hand"}),
    );
    w.ok(
        Operation::StartMessageMoveTask,
        &json!({"SourceArn": dlq_arn}),
    );
    w.clock.advance(1_000);
    let task = last_task(&mut w, &dlq_arn);
    assert_eq!(task["Status"], "FAILED");
    assert_eq!(task["ApproximateNumberOfMessagesMoved"], 1);
    assert_eq!(task["ApproximateNumberOfMessagesToMove"], 2);
}
