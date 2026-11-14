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
