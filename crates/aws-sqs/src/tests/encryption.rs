//! Which server-side encryption a queue reports: SQS-managed by default, and not beside a KMS key.

use super::*;

/// A queue with a KMS key is SSE-KMS: only one server-side encryption option is supported per queue, so SQS-managed encryption reads
/// as off beside the key, and as on for a queue with none.
#[test]
fn a_queue_with_a_kms_key_does_not_report_sqs_managed_encryption() {
    let mut w = World::new();
    let plain = w.queue("plain");
    let keyed = w.ok(
        Operation::CreateQueue,
        &json!({"QueueName": "keyed", "Attributes": {"KmsMasterKeyId": "alias/larder-app"}}),
    )["QueueUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    let read = |w: &mut World, url: &str| {
        w.ok(
            Operation::GetQueueAttributes,
            &json!({"QueueUrl": url, "AttributeNames": ["All"]}),
        )["Attributes"]
            .clone()
    };
    assert_eq!(read(&mut w, &plain)["SqsManagedSseEnabled"], "true");
    let attrs = read(&mut w, &keyed);
    assert_eq!(attrs["KmsMasterKeyId"], "alias/larder-app");
    assert_eq!(attrs["SqsManagedSseEnabled"], "false");
}
