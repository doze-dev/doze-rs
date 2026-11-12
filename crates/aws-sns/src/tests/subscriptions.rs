//! Subscriptions: who confirms, attributes, filters being checked, unsubscribing and listing.

use serde_json::json;

use super::{ACCOUNT, World, arn};

const QUEUE: &str = "arn:aws:sqs:us-east-1:000000000000:q";

#[test]
fn a_queue_is_confirmed_at_once_and_the_attributes_are_those_aws_recorded() {
    let mut w = World::new();
    let topic = w.topic("t");
    let made = w.ok(
        "Subscribe",
        &json!({"TopicArn": topic, "Protocol": "sqs", "Endpoint": QUEUE}),
    );
    let sub = made["SubscriptionArn"].as_str().unwrap();
    assert!(sub.starts_with(&format!("{topic}:")), "{sub}");
    let got = w.ok(
        "GetSubscriptionAttributes",
        &json!({"SubscriptionArn": sub}),
    )["Attributes"]
        .clone();
    for (k, v) in [
        ("PendingConfirmation", "false"),
        ("ConfirmationWasAuthenticated", "true"),
        ("RawMessageDelivery", "false"),
        ("Protocol", "sqs"),
        ("Endpoint", QUEUE),
        ("Owner", ACCOUNT),
        ("TopicArn", topic.as_str()),
        ("SubscriptionArn", sub),
    ] {
        assert_eq!(got[k], v, "{k}");
    }
    assert!(
        got["SubscriptionPrincipal"]
            .as_str()
            .unwrap()
            .starts_with("arn:aws:iam::")
    );
    assert!(got.get("FilterPolicy").is_none() && got.get("FilterPolicyScope").is_none());
    let counts = w.ok("GetTopicAttributes", &json!({"TopicArn": topic}));
    assert_eq!(counts["Attributes"]["SubscriptionsConfirmed"], "1");
}

#[test]
fn subscribing_the_same_endpoint_again_is_the_same_subscription() {
    let mut w = World::new();
    let topic = w.topic("t");
    let one = w.subscribe_queue(&topic, QUEUE, &json!({}));
    let two = w.subscribe_queue(&topic, QUEUE, &json!({}));
    assert_eq!(one, two);
    let different = json!({"TopicArn": topic, "Protocol": "sqs", "Endpoint": QUEUE, "Attributes": {"RawMessageDelivery": "true"}});
    assert!(
        w.refused("Subscribe", &different)
            .contains("different attributes")
    );
    assert_eq!(w.core.subs.len(), 1);
}

#[test]
fn an_endpoint_that_cannot_confirm_waits_and_a_token_confirms_it() {
    let mut w = World::new();
    let topic = w.topic("t");
    let said = w.ok(
        "Subscribe",
        &json!({"TopicArn": topic, "Protocol": "https", "Endpoint": "https://example.com/hook"}),
    );
    assert_eq!(said["SubscriptionArn"], "pending confirmation");
    let asked = w.ok("Subscribe", &json!({"TopicArn": topic, "Protocol": "email", "Endpoint": "a@example.com", "ReturnSubscriptionArn": true}));
    let arn_of_email = asked["SubscriptionArn"].as_str().unwrap().to_owned();
    assert!(arn_of_email.starts_with("arn:aws:sns:"));
    // Pending until confirmed, and counted so.
    let attrs = w.ok(
        "GetSubscriptionAttributes",
        &json!({"SubscriptionArn": arn_of_email}),
    )["Attributes"]
        .clone();
    assert_eq!(
        (
            &attrs["PendingConfirmation"],
            &attrs["ConfirmationWasAuthenticated"]
        ),
        (&json!("true"), &json!("false"))
    );
    assert_eq!(
        w.ok("GetTopicAttributes", &json!({"TopicArn": topic}))["Attributes"]["SubscriptionsPending"],
        "2"
    );
    let listed = w.ok("ListSubscriptionsByTopic", &json!({"TopicArn": topic}));
    assert_eq!(
        listed["Subscriptions"][0]["SubscriptionArn"],
        "PendingConfirmation"
    );
    confirm_with(&mut w, &topic, &arn_of_email);
}

/// The token is the one the subscription was given; a wrong one, or one for another topic, confirms nothing.
fn confirm_with(w: &mut World, topic: &str, arn_of_email: &str) {
    let token = w
        .core
        .subs
        .get(arn_of_email)
        .unwrap()
        .token
        .clone()
        .unwrap();
    assert!(
        w.refused(
            "ConfirmSubscription",
            &json!({"TopicArn": topic, "Token": "wrong"})
        )
        .contains("Token")
    );
    let other = w.topic("other");
    assert!(
        w.run(
            "ConfirmSubscription",
            &json!({"TopicArn": other, "Token": token})
        )
        .is_err()
    );
    let confirmed = w.ok(
        "ConfirmSubscription",
        &json!({"TopicArn": topic, "Token": token}),
    );
    assert_eq!(confirmed["SubscriptionArn"], arn_of_email);
    let attrs = w.ok(
        "GetSubscriptionAttributes",
        &json!({"SubscriptionArn": arn_of_email}),
    )["Attributes"]
        .clone();
    assert_eq!(attrs["PendingConfirmation"], "false");
    assert!(
        w.run(
            "ConfirmSubscription",
            &json!({"TopicArn": topic, "Token": token})
        )
        .is_err()
    );
}

#[test]
fn a_token_is_good_for_two_days() {
    let mut w = World::new();
    let topic = w.topic("t");
    let said = w.ok("Subscribe", &json!({"TopicArn": topic, "Protocol": "email", "Endpoint": "a@example.com", "ReturnSubscriptionArn": true}));
    let token = w
        .core
        .subs
        .get(said["SubscriptionArn"].as_str().unwrap())
        .unwrap()
        .token
        .clone()
        .unwrap();
    w.clock.advance(2 * 24 * 60 * 60 * 1000 + 1);
    assert!(
        w.run(
            "ConfirmSubscription",
            &json!({"TopicArn": topic, "Token": token})
        )
        .is_err()
    );
}

#[test]
fn a_queue_of_another_account_must_confirm_and_the_other_protocols_confirm_at_once() {
    let mut w = World::new();
    let topic = w.topic("t");
    let foreign = w.ok("Subscribe", &json!({"TopicArn": topic, "Protocol": "sqs", "Endpoint": "arn:aws:sqs:us-east-1:999999999999:q"}));
    assert_eq!(foreign["SubscriptionArn"], "pending confirmation");
    for (protocol, endpoint) in [
        ("sms", "+14155552671"),
        ("lambda", "arn:aws:lambda:us-east-1:000000000000:function:f"),
    ] {
        let said = w.ok(
            "Subscribe",
            &json!({"TopicArn": topic, "Protocol": protocol, "Endpoint": endpoint}),
        );
        assert_ne!(
            said["SubscriptionArn"], "pending confirmation",
            "{protocol}"
        );
    }
    let firehose = json!({"TopicArn": topic, "Protocol": "firehose", "Endpoint": "arn:aws:firehose:us-east-1:000000000000:deliverystream/d"});
    assert!(w.refused("Subscribe", &firehose).contains("role ARN"));
    let mut with_role = firehose;
    with_role["Attributes"] = json!({"SubscriptionRoleArn": "arn:aws:iam::000000000000:role/r"});
    assert_ne!(
        w.ok("Subscribe", &with_role)["SubscriptionArn"],
        "pending confirmation"
    );
}

#[test]
fn refusals_of_subscribe() {
    let mut w = World::new();
    let topic = w.topic("t");
    let fifo = w.ok(
        "CreateTopic",
        &json!({"Name": "f.fifo", "Attributes": {"FifoTopic": "true"}}),
    )["TopicArn"]
        .clone();
    let sub = |protocol: &str, endpoint: &str| json!({"TopicArn": topic, "Protocol": protocol, "Endpoint": endpoint});
    // Recorded 2026-11-09.
    assert_eq!(
        w.refused("Subscribe", &sub("carrier-pigeon", "x")),
        "Invalid parameter: Amazon SNS does not support this protocol string: carrier-pigeon"
    );
    assert_eq!(
        w.refused(
            "Subscribe",
            &sub("sqs", "arn:aws:sqs:us-east-1:000000000000:q.fifo")
        ),
        "Invalid parameter: Invalid parameter: Endpoint Reason: FIFO SQS Queues can not be subscribed to standard SNS topics"
    );
    assert_eq!(
        w.refused("Subscribe", &json!({"TopicArn": topic, "Protocol": "sqs", "Endpoint": QUEUE, "Attributes": {"FilterPolicy": "{not json"}})),
        "Invalid parameter: Attributes Reason: FilterPolicy: failed to parse JSON. Unexpected character ('n' (code 110)): was expecting double-quote to start field name\n at [Source: REDACTED (`StreamReadFeature.INCLUDE_SOURCE_IN_LOCATION` disabled); line: 1, column: 2]"
    );
    // From memory.
    assert!(
        w.refused(
            "Subscribe",
            &json!({"TopicArn": fifo, "Protocol": "sqs", "Endpoint": QUEUE})
        )
        .contains("Please use FIFO SQS queue")
    );
    for (protocol, endpoint) in [
        ("sqs", "not-an-arn"),
        ("http", "https://x"),
        ("https", "http://x"),
        ("email", "nobody"),
        ("sms", "123"),
        ("lambda", QUEUE),
        (
            "application",
            "arn:aws:sns:us-east-1:000000000000:endpoint/GCM/app/x",
        ),
    ] {
        assert!(
            w.run("Subscribe", &sub(protocol, endpoint)).is_err(),
            "{protocol} {endpoint}"
        );
    }
    assert_eq!(
        w.err(
            "Subscribe",
            &json!({"TopicArn": arn("absent"), "Protocol": "sqs", "Endpoint": QUEUE})
        ),
        ("NotFoundException".into(), "Topic does not exist".into())
    );
    assert_eq!(
        w.refused(
            "Subscribe",
            &json!({"TopicArn": "nonsense", "Protocol": "sqs", "Endpoint": QUEUE})
        ),
        "Invalid parameter: TopicArn Reason: An ARN must have at least 6 elements, not 1"
    );
}

#[test]
fn setting_a_filter_policy_gives_the_scope_and_a_scope_change_rereads_the_policy() {
    let mut w = World::new();
    let topic = w.topic("t");
    let sub = w.subscribe_queue(&topic, QUEUE, &json!({}));
    let set = |w: &mut World, name: &str, value: &str| {
        w.run(
            "SetSubscriptionAttributes",
            &json!({"SubscriptionArn": sub, "AttributeName": name, "AttributeValue": value}),
        )
    };
    set(&mut w, "FilterPolicy", r#"{"kind": ["order"]}"#).unwrap();
    let got = w.ok(
        "GetSubscriptionAttributes",
        &json!({"SubscriptionArn": sub}),
    )["Attributes"]
        .clone();
    assert_eq!(got["FilterPolicy"], r#"{"kind":["order"]}"#);
    assert_eq!(got["FilterPolicyScope"], "MessageAttributes");
    set(&mut w, "FilterPolicy", r#"{"a":{"b":["x"]}}"#).unwrap_err();
    set(&mut w, "FilterPolicyScope", "MessageBody").unwrap();
    set(&mut w, "FilterPolicy", r#"{"a":{"b":["x"]}}"#).unwrap();
    assert!(
        set(&mut w, "FilterPolicyScope", "MessageAttributes").is_err(),
        "a nested policy is not one for attributes"
    );
    set(&mut w, "FilterPolicy", "").unwrap();
    assert!(
        w.ok(
            "GetSubscriptionAttributes",
            &json!({"SubscriptionArn": sub})
        )["Attributes"]
            .get("FilterPolicy")
            .is_none()
    );
    assert_eq!(
        set(&mut w, "Nonsense", "x").unwrap_err().message,
        "Invalid parameter: AttributeName"
    );
    set(&mut w, "RawMessageDelivery", "true").unwrap();
    assert!(set(&mut w, "RawMessageDelivery", "maybe").is_err());
}

#[test]
fn unsubscribing_removes_it_and_counts_it() {
    let mut w = World::new();
    let topic = w.topic("t");
    let sub = w.subscribe_queue(&topic, QUEUE, &json!({}));
    w.ok("Unsubscribe", &json!({"SubscriptionArn": sub}));
    assert_eq!(
        w.ok("GetTopicAttributes", &json!({"TopicArn": topic}))["Attributes"]["SubscriptionsDeleted"],
        "1"
    );
    assert_eq!(
        w.err("Unsubscribe", &json!({"SubscriptionArn": sub})).0,
        "NotFoundException"
    );
    assert!(
        w.refused(
            "GetSubscriptionAttributes",
            &json!({"SubscriptionArn": "pending confirmation"})
        )
        .contains("An ARN must have at least 6")
    );
    assert!(
        w.ok("ListSubscriptions", &json!({}))["Subscriptions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn subscriptions_are_listed_for_the_account_and_for_a_topic() {
    let mut w = World::new();
    let (a, b) = (w.topic("a"), w.topic("b"));
    w.subscribe_queue(&a, "arn:aws:sqs:us-east-1:000000000000:q1", &json!({}));
    w.subscribe_queue(&b, "arn:aws:sqs:us-east-1:000000000000:q2", &json!({}));
    let all = w.ok("ListSubscriptions", &json!({}));
    assert_eq!(all["Subscriptions"].as_array().unwrap().len(), 2);
    let one = w.ok("ListSubscriptionsByTopic", &json!({"TopicArn": a}));
    let listed = &one["Subscriptions"][0];
    assert_eq!(
        (
            &listed["Protocol"],
            &listed["Endpoint"],
            &listed["TopicArn"],
            &listed["Owner"]
        ),
        (
            &json!("sqs"),
            &json!("arn:aws:sqs:us-east-1:000000000000:q1"),
            &json!(a),
            &json!(ACCOUNT)
        )
    );
    assert_eq!(
        w.err(
            "ListSubscriptionsByTopic",
            &json!({"TopicArn": arn("absent")})
        )
        .0,
        "NotFoundException"
    );
}
