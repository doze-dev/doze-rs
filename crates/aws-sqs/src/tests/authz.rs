//! What a request asks the authorizer for, read from its decoded input rather than the body parsed again.

use super::*;

/// A batch is asked for as the single send, on the queue its URL names, with the queue's own policy beside it; a URL sent with
/// escaped slashes reads the same.
#[test]
fn a_batch_send_is_asked_for_as_a_send_on_the_queue_its_url_names() {
    let mut w = World::new();
    let policy = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":"*","Action":"sqs:SendMessage","Resource":"*"}]}"#;
    let url = w.ok(
        Operation::CreateQueue,
        &json!({"QueueName": "jobs", "Attributes": {"Policy": policy}}),
    )["QueueUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    let body = json!({"QueueUrl": url, "Entries": [{"Id": "1", "MessageBody": "x".repeat(4096)}]});
    let call = call_of(&w.patterns, Operation::SendMessageBatch, &body).unwrap();
    let needs = w.core.needs(&call);
    assert_eq!(needs.len(), 1);
    assert_eq!(needs[0].action, "sqs:SendMessage");
    assert_eq!(needs[0].resource, "arn:aws:sqs:us-east-1:000000000000:jobs");
    assert_eq!(needs[0].policy.as_deref(), Some(policy));

    let escaped = format!(r#"{{"QueueUrl":"{}"}}"#, url.replace('/', "\\/"));
    let call = call_of(
        &w.patterns,
        Operation::GetQueueAttributes,
        &serde_json::from_str(&escaped).unwrap(),
    )
    .unwrap();
    assert_eq!(
        w.core.needs(&call)[0].resource,
        "arn:aws:sqs:us-east-1:000000000000:jobs"
    );
}

/// A move task is asked for on the source queue's ARN; creating a queue on the queue it names; listing on every queue.
#[test]
fn moves_creates_and_lists_are_asked_for_on_what_they_name() {
    let w = World::new();
    let needs = |op, body: Value| w.core.needs(&call_of(&w.patterns, op, &body).unwrap());
    let source = "arn:aws:sqs:us-east-1:000000000000:dead";
    let moving = needs(
        Operation::StartMessageMoveTask,
        json!({"SourceArn": source}),
    );
    assert_eq!(
        (moving[0].action.as_str(), moving[0].resource.as_str()),
        ("sqs:StartMessageMoveTask", source)
    );
    let made = needs(Operation::CreateQueue, json!({"QueueName": "new"}));
    assert_eq!(made[0].resource, "arn:aws:sqs:us-east-1:000000000000:new");
    let listed = needs(Operation::ListQueues, json!({}));
    assert_eq!(listed[0].resource, "arn:aws:sqs:us-east-1:000000000000:*");
}
