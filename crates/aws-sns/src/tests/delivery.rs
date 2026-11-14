//! Fan-out to SQS through the peer bus: the envelope, raw delivery, FIFO, protocols that are not delivered, and the queue's policy.

use aws_peers::Mode;
use serde_json::{Value, json};

use super::{ACCOUNT, World};

fn publish(w: &mut World, topic: &str, extra: &Value) -> Value {
    let mut request = json!({"TopicArn": topic, "Message": "shipped"});
    for (k, v) in extra.as_object().unwrap() {
        request[k] = v.clone();
    }
    w.ok("Publish", &request)
}

#[test]
fn a_queue_receives_the_envelope_with_the_fields_aws_recorded() {
    let mut w = World::new();
    let topic = w.topic("t");
    let queue = w.queue("q", &topic, json!({}));
    let sub = w.subscribe_queue(&topic, &queue, &json!({}));
    let sent = publish(
        &mut w,
        &topic,
        &json!({"Subject": "order", "MessageAttributes": {"kind": {"DataType": "String", "StringValue": "order"}}}),
    );
    let got = w.drain("q");
    assert_eq!(got.len(), 1);
    let note: Value = serde_json::from_str(got[0]["Body"].as_str().unwrap()).unwrap();
    assert_eq!(note["Type"], "Notification");
    assert_eq!(note["MessageId"], sent["MessageId"]);
    assert_eq!(note["TopicArn"], topic);
    assert_eq!(note["Subject"], "order");
    assert_eq!(note["Message"], "shipped");
    assert_eq!(note["SignatureVersion"], "1");
    assert_eq!(
        note["MessageAttributes"]["kind"],
        json!({"Type": "String", "Value": "order"})
    );
    assert!(note["Timestamp"].as_str().unwrap().ends_with("Z"));
    assert!(note["UnsubscribeURL"].as_str().unwrap().contains(&sub));
    for field in ["Signature", "SigningCertURL"] {
        assert!(
            note[field].as_str().is_some_and(|s| !s.is_empty()),
            "{field}"
        );
    }
}

#[test]
fn raw_delivery_is_the_message_and_its_attributes_become_queue_attributes() {
    let mut w = World::new();
    let topic = w.topic("t");
    let queue = w.queue("q", &topic, json!({}));
    w.subscribe_queue(&topic, &queue, &json!({"RawMessageDelivery": "true"}));
    publish(
        &mut w,
        &topic,
        &json!({"Subject": "dropped", "MessageAttributes": {
            "kind": {"DataType": "String", "StringValue": "order"},
            "n": {"DataType": "Number", "StringValue": "5"},
            "b": {"DataType": "Binary", "BinaryValue": "AAEC"},
        }}),
    );
    let got = w.drain("q");
    assert_eq!(got[0]["Body"], "shipped");
    let attrs = &got[0]["MessageAttributes"];
    assert_eq!(attrs["kind"]["StringValue"], "order");
    assert_eq!(attrs["n"]["DataType"], "Number");
    assert_eq!(attrs["b"]["BinaryValue"], "AAEC");
}

#[test]
fn each_subscription_gets_its_own_copy() {
    let mut w = World::new();
    let topic = w.topic("t");
    for name in ["q1", "q2"] {
        let queue = w.queue(name, &topic, json!({}));
        w.subscribe_queue(&topic, &queue, &json!({"RawMessageDelivery": "true"}));
    }
    publish(&mut w, &topic, &json!({}));
    assert_eq!((w.drain("q1").len(), w.drain("q2").len()), (1, 1));
}

#[test]
fn a_json_structure_delivers_the_member_for_sqs_or_default() {
    let mut w = World::new();
    let topic = w.topic("t");
    let queue = w.queue("q", &topic, json!({}));
    w.subscribe_queue(&topic, &queue, &json!({"RawMessageDelivery": "true"}));
    publish(
        &mut w,
        &topic,
        &json!({"Message": r#"{"default":"d","sqs":"for the queue","email":"e"}"#, "MessageStructure": "json"}),
    );
    publish(
        &mut w,
        &topic,
        &json!({"Message": r#"{"default":"d","email":"e"}"#, "MessageStructure": "json"}),
    );
    let bodies: Vec<String> = w
        .drain("q")
        .iter()
        .map(|m| m["Body"].as_str().unwrap().to_owned())
        .collect();
    assert!(
        bodies.contains(&"for the queue".to_owned()) && bodies.contains(&"d".to_owned()),
        "{bodies:?}"
    );
}

#[test]
fn a_fifo_topic_delivers_its_group_and_deduplication_to_a_fifo_queue() {
    let mut w = World::new();
    let topic = w.ok("CreateTopic", &json!({"Name": "t.fifo", "Attributes": {"FifoTopic": "true", "ContentBasedDeduplication": "true"}}))["TopicArn"]
        .as_str()
        .unwrap()
        .to_owned();
    let queue = w.queue("q.fifo", &topic, json!({"FifoQueue": "true"}));
    w.subscribe_queue(&topic, &queue, &json!({"RawMessageDelivery": "true"}));
    let send = json!({"TopicArn": topic, "Message": "first", "MessageGroupId": "g"});
    w.ok("Publish", &send);
    w.ok("Publish", &send);
    let got = w.drain("q.fifo");
    assert_eq!(got.len(), 1, "the duplicate is not delivered");
    assert_eq!(got[0]["Body"], "first");
    assert_eq!(got[0]["Attributes"]["MessageGroupId"], "g");
}

#[test]
fn a_batch_delivers_each_message() {
    let mut w = World::new();
    let topic = w.topic("t");
    let queue = w.queue("q", &topic, json!({}));
    w.subscribe_queue(&topic, &queue, &json!({"RawMessageDelivery": "true"}));
    w.ok("PublishBatch", &json!({"TopicArn": topic, "PublishBatchRequestEntries": [{"Id": "a", "Message": "one"}, {"Id": "b", "Message": "two"}]}));
    let mut bodies: Vec<String> = w
        .drain("q")
        .iter()
        .map(|m| m["Body"].as_str().unwrap().to_owned())
        .collect();
    bodies.sort();
    assert_eq!(bodies, ["one", "two"]);
}

#[test]
fn a_subscription_that_waits_for_confirmation_gets_nothing() {
    let mut w = World::new();
    let topic = w.topic("t");
    let queue = w.queue("q", &topic, json!({}));
    // A queue of another account waits for its owner to confirm.
    w.ok("Subscribe", &json!({"TopicArn": topic, "Protocol": "sqs", "Endpoint": queue.replace(ACCOUNT, "999999999999")}));
    publish(&mut w, &topic, &json!({}));
    assert!(w.drain("q").is_empty());
}

#[test]
fn protocols_that_are_not_delivered_to_do_not_stop_the_ones_that_are() {
    let mut w = World::new();
    let topic = w.topic("t");
    w.ok(
        "Subscribe",
        &json!({"TopicArn": topic, "Protocol": "sms", "Endpoint": "+14155552671"}),
    );
    w.ok("Subscribe", &json!({"TopicArn": topic, "Protocol": "lambda", "Endpoint": "arn:aws:lambda:us-east-1:000000000000:function:f"}));
    let queue = w.queue("q", &topic, json!({}));
    w.subscribe_queue(&topic, &queue, &json!({"RawMessageDelivery": "true"}));
    publish(&mut w, &topic, &json!({}));
    assert_eq!(w.drain("q").len(), 1);
}

#[test]
fn a_queue_that_does_not_exist_loses_the_message_and_not_the_publish() {
    let mut w = World::new();
    let topic = w.topic("t");
    w.subscribe_queue(
        &topic,
        "arn:aws:sqs:us-east-1:000000000000:ghost",
        &json!({}),
    );
    let queue = w.queue("q", &topic, json!({}));
    w.subscribe_queue(&topic, &queue, &json!({"RawMessageDelivery": "true"}));
    publish(&mut w, &topic, &json!({}));
    assert_eq!(w.drain("q").len(), 1);
}

#[test]
fn a_queue_that_does_not_let_the_topic_send_gets_nothing_when_policies_are_enforced() {
    let mut w = World::new();
    let topic = w.topic("t");
    // No policy at all, and a policy for another topic.
    let bare = w.sqs("CreateQueue", &json!({"QueueName": "bare"})).unwrap();
    assert!(bare["QueueUrl"].is_string());
    let other = w.queue(
        "other",
        "arn:aws:sns:us-east-1:000000000000:elsewhere",
        json!({}),
    );
    let allowed = w.queue("allowed", &topic, json!({}));
    for (name, arn) in [
        ("bare", "arn:aws:sqs:us-east-1:000000000000:bare".to_owned()),
        ("other", other),
        ("allowed", allowed),
    ] {
        let _ = name;
        w.subscribe_queue(&topic, &arn, &json!({"RawMessageDelivery": "true"}));
    }
    // Soft: it would be refused, and goes through (the log says so).
    w.peers.set_mode(Mode::Soft);
    publish(&mut w, &topic, &json!({}));
    assert_eq!(
        (
            w.drain("bare").len(),
            w.drain("other").len(),
            w.drain("allowed").len()
        ),
        (1, 1, 1)
    );
    // Enforce: only the queue whose policy names this topic takes it.
    w.peers.set_mode(Mode::Enforce);
    publish(&mut w, &topic, &json!({}));
    assert_eq!(
        (
            w.drain("bare").len(),
            w.drain("other").len(),
            w.drain("allowed").len()
        ),
        (0, 0, 1)
    );
    // Off: nothing is asked.
    w.peers.set_mode(Mode::Off);
    publish(&mut w, &topic, &json!({}));
    assert_eq!(
        (
            w.drain("bare").len(),
            w.drain("other").len(),
            w.drain("allowed").len()
        ),
        (1, 1, 1)
    );
}

#[test]
fn a_message_that_cannot_be_delivered_goes_to_the_dead_letter_queue_with_why() {
    let mut w = World::new();
    let topic = w.topic("t");
    let dlq = w.queue("dlq", &topic, json!({}));
    w.subscribe_queue(
        &topic,
        "arn:aws:sqs:us-east-1:000000000000:ghost",
        &json!({"RawMessageDelivery": "true", "RedrivePolicy": json!({"deadLetterTargetArn": dlq}).to_string()}),
    );
    publish(
        &mut w,
        &topic,
        &json!({"MessageAttributes": {"kind": {"DataType": "String", "StringValue": "order"}}}),
    );
    let got = w.drain("dlq");
    assert_eq!(got.len(), 1);
    assert_eq!(
        got[0]["Body"], "shipped",
        "the body the subscription would have been given"
    );
    let attrs = &got[0]["MessageAttributes"];
    assert_eq!(attrs["kind"]["StringValue"], "order");
    assert_eq!(attrs["ERROR_CODE"]["StringValue"], "QueueDoesNotExist");
    assert!(attrs["ERROR_MESSAGE"]["StringValue"].is_string());
    assert!(attrs["RequestID"]["StringValue"].is_string());
    assert_eq!(attrs["TopicArn"]["StringValue"], topic);
    // A subscription with no redrive policy loses the message, as before.
    let bare = w.ok("ListSubscriptionsByTopic", &json!({"TopicArn": topic}));
    assert_eq!(bare["Subscriptions"].as_array().unwrap().len(), 1);
}
