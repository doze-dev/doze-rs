//! Publish and PublishBatch: what is checked, FIFO topics, batches, and the messages that are accepted and sent nowhere.

use serde_json::{Value, json};

use super::{World, arn};

#[test]
fn what_a_publish_checks_is_worded_as_aws_worded_it() {
    let mut w = World::new();
    let topic = w.topic("t");
    // Recorded 2026-11-09.
    assert_eq!(
        w.refused("Publish", &json!({"TopicArn": topic, "Message": ""})),
        "Invalid parameter: Empty message"
    );
    assert_eq!(
        w.err(
            "Publish",
            &json!({"TopicArn": arn("absent"), "Message": "x"})
        ),
        ("NotFoundException".into(), "Topic does not exist".into())
    );
    assert_eq!(
        w.refused("Publish", &json!({"TopicArn": "nonsense", "Message": "x"})),
        "Invalid parameter: TopicArn Reason: An ARN must have at least 6 elements, not 1"
    );
}

#[test]
fn what_a_publish_checks_that_aws_was_not_asked_about() {
    let mut w = World::new();
    let topic = w.topic("t");
    // From memory.
    assert_eq!(
        w.refused("Publish", &json!({"Message": "x"})),
        "Invalid parameter: TopicArn or TargetArn Reason: no value for required parameter"
    );
    assert!(
        w.refused(
            "Publish",
            &json!({"TopicArn": topic, "Message": "x", "Subject": "a\nb"})
        )
        .contains("Subject")
    );
    assert!(
        w.run(
            "Publish",
            &json!({"TopicArn": topic, "Message": "x", "Subject": "s".repeat(101)})
        )
        .is_err()
    );
    assert!(
        w.run(
            "Publish",
            &json!({"TopicArn": topic, "Message": "x", "Subject": "s".repeat(100)})
        )
        .is_ok()
    );
    let eleven: Value = (0..11)
        .map(|i| {
            (
                format!("a{i}"),
                json!({"DataType": "String", "StringValue": "v"}),
            )
        })
        .collect::<serde_json::Map<_, _>>()
        .into();
    assert!(
        w.refused(
            "Publish",
            &json!({"TopicArn": topic, "Message": "x", "MessageAttributes": eleven})
        )
        .contains("exceeds the allowed maximum [10]")
    );
    assert!(
        w.refused(
            "Publish",
            &json!({"TopicArn": topic, "Message": "x".repeat(262_145)})
        )
        .ends_with("Message too long")
    );
    assert!(
        w.run(
            "Publish",
            &json!({"TopicArn": topic, "Message": "x".repeat(262_144)})
        )
        .is_ok()
    );
}

#[test]
fn a_topic_raised_to_a_megabyte_takes_a_megabyte() {
    let mut w = World::new();
    let topic = w.topic("t");
    w.ok("SetTopicAttributes", &json!({"TopicArn": topic, "AttributeName": "MaximumMessageSize", "AttributeValue": "1048576"}));
    assert!(
        w.run(
            "Publish",
            &json!({"TopicArn": topic, "Message": "x".repeat(1_048_576)})
        )
        .is_ok()
    );
    assert!(
        w.run(
            "Publish",
            &json!({"TopicArn": topic, "Message": "x".repeat(1_048_577)})
        )
        .is_err()
    );
}

#[test]
fn a_fifo_topic_wants_a_group_and_deduplicates_for_five_minutes() {
    let mut w = World::new();
    let topic = w.ok(
        "CreateTopic",
        &json!({"Name": "f.fifo", "Attributes": {"FifoTopic": "true"}}),
    )["TopicArn"]
        .clone();
    // Recorded 2026-11-09.
    assert_eq!(
        w.refused("Publish", &json!({"TopicArn": topic, "Message": "x"})),
        "Invalid parameter: The MessageGroupId parameter is required for FIFO topics"
    );
    // From memory.
    assert!(
        w.refused(
            "Publish",
            &json!({"TopicArn": topic, "Message": "x", "MessageGroupId": "g"})
        )
        .contains("ContentBasedDeduplication")
    );
    let send = json!({"TopicArn": topic, "Message": "x", "MessageGroupId": "g", "MessageDeduplicationId": "d1"});
    let first = w.ok("Publish", &send);
    assert!(first["SequenceNumber"].as_str().unwrap().starts_with('1'));
    assert_eq!(first["SequenceNumber"].as_str().unwrap().len(), 20);
    let again = w.ok("Publish", &send);
    assert_eq!(again, first, "a duplicate gets the original's answer");
    let other = w.ok("Publish", &json!({"TopicArn": topic, "Message": "y", "MessageGroupId": "g", "MessageDeduplicationId": "d2"}));
    assert_ne!(other["MessageId"], first["MessageId"]);
    assert!(other["SequenceNumber"].as_str().unwrap() > first["SequenceNumber"].as_str().unwrap());
    w.clock.advance(5 * 60 * 1000 + 1);
    let later = w.ok("Publish", &send);
    assert_ne!(
        later["MessageId"], first["MessageId"],
        "forgotten after five minutes"
    );
}

#[test]
fn content_based_deduplication_hashes_the_body_and_a_standard_topic_refuses_a_deduplication_id() {
    let mut w = World::new();
    let topic = w.ok("CreateTopic", &json!({"Name": "f.fifo", "Attributes": {"FifoTopic": "true", "ContentBasedDeduplication": "true"}}))["TopicArn"].clone();
    let send = |body: &str| json!({"TopicArn": topic, "Message": body, "MessageGroupId": "g"});
    let one = w.ok("Publish", &send("same"));
    assert_eq!(
        w.ok("Publish", &send("same"))["MessageId"],
        one["MessageId"]
    );
    assert_ne!(
        w.ok("Publish", &send("different"))["MessageId"],
        one["MessageId"]
    );
    let standard = w.topic("s");
    assert!(
        w.refused(
            "Publish",
            &json!({"TopicArn": standard, "Message": "x", "MessageDeduplicationId": "d"})
        )
        .contains("not valid for this topic type")
    );
    // A group id on a standard topic is allowed (it goes to standard queues only).
    assert!(
        w.run(
            "Publish",
            &json!({"TopicArn": standard, "Message": "x", "MessageGroupId": "g"})
        )
        .is_ok()
    );
}

#[test]
fn a_batch_is_checked_whole_and_each_message_answers_for_itself() {
    let mut w = World::new();
    let topic = w.topic("t");
    let entry = |id: &str, body: &str| json!({"Id": id, "Message": body});
    let batch =
        |entries: Vec<Value>| json!({"TopicArn": topic, "PublishBatchRequestEntries": entries});
    // Recorded 2026-11-09.
    assert_eq!(
        w.err("PublishBatch", &batch(vec![])),
        (
            "EmptyBatchRequestException".into(),
            "The batch request doesn't contain any entries.".into()
        )
    );
    let eleven: Vec<Value> = (0..11).map(|i| entry(&format!("m{i}"), "x")).collect();
    assert_eq!(
        w.err("PublishBatch", &batch(eleven)),
        (
            "TooManyEntriesInBatchRequestException".into(),
            "The batch request contains more entries than permissible.".into()
        )
    );
    assert_eq!(
        w.err(
            "PublishBatch",
            &batch(vec![entry("a", "1"), entry("a", "2")])
        ),
        (
            "BatchEntryIdsNotDistinctException".into(),
            "Two or more batch entries in the request have the same Id.".into()
        )
    );
    // From memory.
    assert_eq!(
        w.err("PublishBatch", &batch(vec![entry("a b", "1")])).0,
        "InvalidBatchEntryIdException"
    );
    let big = "x".repeat(150_000);
    assert_eq!(
        w.err(
            "PublishBatch",
            &batch(vec![entry("a", &big), entry("b", &big)])
        )
        .0,
        "BatchRequestTooLongException"
    );
    let done = w.ok(
        "PublishBatch",
        &batch(vec![entry("a", "one"), entry("b", ""), entry("c", "three")]),
    );
    let ok: Vec<&str> = done["Successful"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["Id"].as_str().unwrap())
        .collect();
    assert_eq!(ok, ["a", "c"]);
    assert_eq!(
        done["Failed"],
        json!([{"Id": "b", "Code": "InvalidParameter", "Message": "Invalid parameter: Empty message", "SenderFault": true}])
    );
}

#[test]
fn a_text_message_and_a_push_message_are_accepted_and_sent_nowhere() {
    let mut w = World::new();
    let sent = w.ok(
        "Publish",
        &json!({"PhoneNumber": "+14155552671", "Message": "hello"}),
    );
    assert!(sent["MessageId"].as_str().is_some_and(|id| id.len() == 36));
    assert!(
        w.refused(
            "Publish",
            &json!({"PhoneNumber": "12345", "Message": "hello"})
        )
        .contains("is not valid to publish to")
    );
    let app = w.ok(
        "CreatePlatformApplication",
        &json!({"Name": "app", "Platform": "GCM", "Attributes": {"PlatformCredential": "key"}}),
    );
    let endpoint = w.ok(
        "CreatePlatformEndpoint",
        &json!({"PlatformApplicationArn": app["PlatformApplicationArn"], "Token": "tok"}),
    );
    let target = endpoint["EndpointArn"].clone();
    assert!(
        w.ok("Publish", &json!({"TargetArn": target, "Message": "hi"}))["MessageId"].is_string()
    );
    w.ok(
        "SetEndpointAttributes",
        &json!({"EndpointArn": target, "Attributes": {"Enabled": "false"}}),
    );
    assert_eq!(
        w.err("Publish", &json!({"TargetArn": target, "Message": "hi"}))
            .0,
        "EndpointDisabledException"
    );
    let gone = "arn:aws:sns:us-east-1:000000000000:endpoint/GCM/app/none";
    assert_eq!(
        w.err("Publish", &json!({"TargetArn": gone, "Message": "hi"}))
            .0,
        "NotFoundException"
    );
    // A topic ARN in `TargetArn` is a topic.
    let topic = w.topic("t");
    assert!(
        w.ok("Publish", &json!({"TargetArn": topic, "Message": "hi"}))["MessageId"].is_string()
    );
}
