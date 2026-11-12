//! Filter policies at delivery: on message attributes and, with `FilterPolicyScope: MessageBody`, on the body. The matching rules have
//! their own tests; these show that a subscription holds them through a publish.

use serde_json::{Value, json};

use super::World;

/// A topic with one raw queue subscription holding `policy` in `scope`.
fn setup(policy: &str, scope: &str) -> (World, String) {
    let mut w = World::new();
    let topic = w.topic("t");
    let queue = w.queue("q", &topic, json!({}));
    w.subscribe_queue(
        &topic,
        &queue,
        &json!({"RawMessageDelivery": "true", "FilterPolicy": policy, "FilterPolicyScope": scope}),
    );
    (w, topic)
}

fn attr(kind: &str, value: &str) -> Value {
    json!({"kind": {"DataType": kind, "StringValue": value}})
}

fn delivered(w: &mut World, topic: &str, message: &str, attributes: &Value) -> bool {
    let mut request = json!({"TopicArn": topic, "Message": message});
    if !attributes.is_null() {
        request["MessageAttributes"] = attributes.clone();
    }
    w.ok("Publish", &request);
    !w.drain("q").is_empty()
}

#[test]
fn a_message_passes_when_its_attributes_match_and_not_otherwise() {
    let (mut w, topic) = setup(r#"{"kind":["order"]}"#, "MessageAttributes");
    // Recorded 2026-11-09: the message with the other value is not delivered, the one with the value is.
    assert!(!delivered(
        &mut w,
        &topic,
        "filtered out",
        &attr("String", "refund")
    ));
    assert!(delivered(
        &mut w,
        &topic,
        "let through",
        &attr("String", "order")
    ));
    assert!(!delivered(&mut w, &topic, "no attributes", &Value::Null));
}

#[test]
fn the_body_scope_reads_the_message_as_json() {
    let (mut w, topic) = setup(
        r#"{"order":{"kind":["rush"],"qty":[{"numeric":[">",1]}]}}"#,
        "MessageBody",
    );
    assert!(delivered(
        &mut w,
        &topic,
        r#"{"order":{"kind":"rush","qty":3}}"#,
        &Value::Null
    ));
    assert!(!delivered(
        &mut w,
        &topic,
        r#"{"order":{"kind":"rush","qty":1}}"#,
        &Value::Null
    ));
    assert!(!delivered(&mut w, &topic, "plain text", &Value::Null));
    // The attributes mean nothing to a body policy.
    assert!(!delivered(
        &mut w,
        &topic,
        "plain text",
        &attr("String", "order")
    ));
}

#[test]
fn a_body_policy_does_not_drop_what_an_attribute_policy_would_pass() {
    // SCOPE.md, row (b): the Go edition matched a body policy against attributes and dropped every message. Here the scope is read.
    let (mut w, topic) = setup(r#"{"colour":["red"]}"#, "MessageBody");
    assert!(delivered(
        &mut w,
        &topic,
        r#"{"colour":"red"}"#,
        &Value::Null
    ));
}

#[test]
fn the_operators_hold_through_a_subscription() {
    let cases: [(&str, &str, &str, bool); 7] = [
        (r#"{"kind":[{"prefix":"ord"}]}"#, "String", "order", true),
        (
            r#"{"kind":[{"anything-but":["order"]}]}"#,
            "String",
            "order",
            false,
        ),
        (r#"{"kind":[{"numeric":["<=",5]}]}"#, "Number", "5", true),
        (r#"{"kind":[{"exists":true}]}"#, "String", "x", true),
        (
            r#"{"$or":[{"kind":["a"]},{"other":["b"]}]}"#,
            "String",
            "a",
            true,
        ),
        (
            r#"{"kind":[{"equals-ignore-case":"ORDER"}]}"#,
            "String",
            "order",
            true,
        ),
        (
            r#"{"kind":["order"]}"#,
            "String.Array",
            r#"["x","order"]"#,
            true,
        ),
    ];
    for (policy, kind, value, expected) in cases {
        let (mut w, topic) = setup(policy, "MessageAttributes");
        assert_eq!(
            delivered(&mut w, &topic, "m", &attr(kind, value)),
            expected,
            "{policy} {kind} {value}"
        );
    }
}
