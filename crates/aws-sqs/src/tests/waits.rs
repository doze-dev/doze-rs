//! The 60-second waits after a delete and a purge.

use aws_model_sqs::Operation;
use serde_json::json;

use super::World;

/// AWS's 60-second waits are on by default, in AWS's words with doze's sentence after them; `--sqs-skip-60s-waits` turns them off.
#[test]
fn the_sixty_second_waits_are_aws_s_unless_skipped() {
    let mut w = World::new();
    let url = w.queue("q");
    w.ok(Operation::PurgeQueue, &json!({"QueueUrl": url}));
    let (_, shape, message) = w.err(Operation::PurgeQueue, &json!({"QueueUrl": url}));
    assert_eq!(shape, "PurgeQueueInProgress");
    assert!(
        message.starts_with(
            "Only one PurgeQueue operation on q is allowed every 60 seconds. doze-aws: "
        )
    );
    assert!(message.contains("--sqs-skip-60s-waits"));
    w.ok(Operation::DeleteQueue, &json!({"QueueUrl": url}));
    let (_, shape, message) = w.err(Operation::CreateQueue, &json!({"QueueName": "q"}));
    assert_eq!(shape, "QueueDeletedRecently");
    assert!(message.starts_with("You must wait 60 seconds after deleting a queue before you can create another with the same name. doze-aws: "));

    w.core.skip_waits = true;
    let url = w.queue("q");
    w.ok(Operation::PurgeQueue, &json!({"QueueUrl": url}));
    w.ok(Operation::PurgeQueue, &json!({"QueueUrl": url}));
    w.ok(Operation::DeleteQueue, &json!({"QueueUrl": url}));
    w.queue("q");
}
