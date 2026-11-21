//! A topic's and a subscription's pages of their own: what the topic page reads of its attributes, and the operations its pages make
//! (a topic's settings and Publish, a subscription's settings, Subscribe with its attributes, ConfirmSubscription), each bound to the
//! resource it acts on. Against real services on a real bus.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]

mod support;
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use serde_json::{Value, json};
use world::{encode, field, world};

const BASE: &str = "/api/v1/resources";
const ACCOUNT: &str = "000000000000";

fn topic_arn(name: &str) -> String {
    format!("arn:aws:sns:us-east-1:{ACCOUNT}:{name}")
}

#[tokio::test]
async fn a_topic_is_made_with_its_attributes_and_tags_and_its_page_reads_them_back() {
    let w = world("sns-create", Mode::Enforce, &[]);
    let (status, made) = w
        .send(
            "POST",
            &format!("{BASE}/sns/topics/op/CreateTopic"),
            &json!({"Name": "orders-events.fifo",
                "Attributes": {"FifoTopic": "true", "ContentBasedDeduplication": "true", "FifoThroughputScope": "MessageGroup",
                    "DisplayName": "larder orders", "MaximumMessageSize": "524288", "KmsMasterKeyId": "alias/larder-app",
                    "TracingConfig": "Active",
                    "DeliveryPolicy": "{\"http\":{\"defaultHealthyRetryPolicy\":{\"minDelayTarget\":5,\"maxDelayTarget\":50,\"numRetries\":7}}}",
                    "SQSSuccessFeedbackRoleArn": "arn:aws:iam::000000000000:role/sns-delivery-logs",
                    "SQSSuccessFeedbackSampleRate": "100",
                    "SQSFailureFeedbackRoleArn": "arn:aws:iam::000000000000:role/sns-delivery-logs"},
                "Tags": [{"Key": "stack", "Value": "larder"}, {"Key": "app", "Value": "fulfilment"}]}),
        )
        .await;
    assert_eq!(status, 200, "{made}");
    assert_eq!(made["TopicArn"], topic_arn("orders-events.fifo"));
    let (status, d) = w
        .get(&format!(
            "{BASE}/sns/topics/{}",
            encode("orders-events.fifo")
        ))
        .await;
    assert_eq!(status, 200, "{d}");
    for (label, value) in [
        ("Type", "FIFO"),
        ("Content-based deduplication", "Enabled"),
        ("FIFO throughput scope", "MessageGroup"),
        ("Display name", "larder orders"),
        ("Maximum message size", "524288"),
        ("KMS key", "alias/larder-app"),
        ("Active tracing", "Active"),
        ("SQSSuccessFeedbackSampleRate", "100"),
        (
            "SQSSuccessFeedbackRoleArn",
            "arn:aws:iam::000000000000:role/sns-delivery-logs",
        ),
        (
            "SQSFailureFeedbackRoleArn",
            "arn:aws:iam::000000000000:role/sns-delivery-logs",
        ),
    ] {
        assert_eq!(field(&d, label)["value"], value, "{label}");
    }
    // The delivery policy the topic was given, apart from the one in force (the defaults with it laid over them).
    let own: Value =
        serde_json::from_str(field(&d, "Own delivery policy")["value"].as_str().unwrap()).unwrap();
    assert_eq!(
        own["http"]["defaultHealthyRetryPolicy"]["numRetries"],
        json!(7)
    );
    let effective: Value =
        serde_json::from_str(field(&d, "Delivery policy")["value"].as_str().unwrap()).unwrap();
    assert_eq!(
        effective["http"]["defaultHealthyRetryPolicy"]["numNoDelayRetries"],
        json!(0)
    );
    // The tags went with the create.
    let (_, tags) = w
        .get(&format!(
            "{BASE}/sns/topics/{}/tags",
            encode("orders-events.fifo")
        ))
        .await;
    let mut got: Vec<String> = tags["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            format!(
                "{}={}",
                t["key"].as_str().unwrap(),
                t["value"].as_str().unwrap()
            )
        })
        .collect();
    got.sort();
    assert_eq!(got, ["app=fulfilment", "stack=larder"]);
    // A name a FIFO topic may not have is the service's refusal, as it words it.
    let (status, refused) = w
        .send(
            "POST",
            &format!("{BASE}/sns/topics/op/CreateTopic"),
            &json!({"Name": "orders-events", "Attributes": {"FifoTopic": "true"}}),
        )
        .await;
    assert_eq!(status, 400, "{refused}");
    assert_eq!(refused["code"], "InvalidParameterException");
    // CreateTopic is idempotent: the same name and attributes answer the topic that is there.
    let (status, again) = w
        .send(
            "POST",
            &format!("{BASE}/sns/topics/op/CreateTopic"),
            &json!({"Name": "orders-events.fifo", "Attributes": {"FifoTopic": "true", "ContentBasedDeduplication": "true",
                "FifoThroughputScope": "MessageGroup", "DisplayName": "larder orders", "MaximumMessageSize": "524288",
                "KmsMasterKeyId": "alias/larder-app", "TracingConfig": "Active",
                "DeliveryPolicy": "{\"http\":{\"defaultHealthyRetryPolicy\":{\"minDelayTarget\":5,\"maxDelayTarget\":50,\"numRetries\":7}}}",
                "SQSSuccessFeedbackRoleArn": "arn:aws:iam::000000000000:role/sns-delivery-logs",
                "SQSSuccessFeedbackSampleRate": "100",
                "SQSFailureFeedbackRoleArn": "arn:aws:iam::000000000000:role/sns-delivery-logs"}}),
        )
        .await;
    assert_eq!(status, 200, "{again}");
    assert_eq!(again["TopicArn"], topic_arn("orders-events.fifo"));
}

#[tokio::test]
async fn a_topics_settings_are_set_one_attribute_a_call_and_a_message_is_published_to_this_topic_alone()
 {
    let w = world("sns-ops", Mode::Soft, &[]);
    for t in ["orders-created", "alerts"] {
        w.app("sns", "CreateTopic", &json!({"Name": t})).await;
    }
    w.app("sqs", "CreateQueue", &json!({"QueueName": "invoice-queue"}))
        .await;
    w.app(
        "sns",
        "Subscribe",
        &json!({"TopicArn": topic_arn("orders-created"), "Protocol": "sqs",
            "Endpoint": format!("arn:aws:sqs:us-east-1:{ACCOUNT}:invoice-queue")}),
    )
    .await;
    let one = format!("{BASE}/sns/topics/orders-created");
    // SetTopicAttributes takes one attribute; the topic it acts on is this one, whatever the page sends.
    let (status, set) = w
        .send(
            "POST",
            &format!("{one}/op/SetTopicAttributes"),
            &json!({"TopicArn": topic_arn("alerts"), "AttributeName": "DisplayName", "AttributeValue": "larder orders"}),
        )
        .await;
    assert_eq!(status, 200, "{set}");
    let (_, d) = w.get(&one).await;
    assert_eq!(field(&d, "Display name")["value"], "larder orders");
    let (_, other) = w.get(&format!("{BASE}/sns/topics/alerts")).await;
    assert_eq!(field(&other, "Display name")["value"], Value::Null);
    // A setting the service refuses is its refusal, with its words.
    let (status, refused) = w
        .send(
            "POST",
            &format!("{one}/op/SetTopicAttributes"),
            &json!({"AttributeName": "MaximumMessageSize", "AttributeValue": "12"}),
        )
        .await;
    assert_eq!(status, 400, "{refused}");
    // Publish: to this topic; the answer is the service's.
    let (status, sent) = w
        .send(
            "POST",
            &format!("{one}/op/Publish"),
            &json!({"TopicArn": topic_arn("alerts"), "Message": "Order 10482 placed", "Subject": "Order",
                "MessageAttributes": {"event": {"DataType": "String", "StringValue": "order_placed"}}}),
        )
        .await;
    assert_eq!(status, 200, "{sent}");
    assert!(sent["MessageId"].as_str().is_some_and(|m| !m.is_empty()));
    let got = w
        .app(
            "sqs",
            "ReceiveMessage",
            &json!({"QueueUrl": format!("http://127.0.0.1:4566/{ACCOUNT}/invoice-queue")}),
        )
        .await;
    // The queue is subscribed to orders-created, so the one message is its, not alerts'.
    let body = got["Messages"][0]["Body"].as_str().unwrap();
    assert!(body.contains("Order 10482 placed"), "{got}");
    // A FIFO topic needs its group; the service says so.
    w.app(
        "sns",
        "CreateTopic",
        &json!({"Name": "events.fifo", "Attributes": {"FifoTopic": "true"}}),
    )
    .await;
    let (status, no_group) = w
        .send(
            "POST",
            &format!("{BASE}/sns/topics/{}/op/Publish", encode("events.fifo")),
            &json!({"Message": "x"}),
        )
        .await;
    assert_eq!(status, 400, "{no_group}");
    // An operation the topic's pages do not make is not made, whatever it names.
    let (status, gone) = w
        .send(
            "POST",
            &format!("{one}/op/DeleteTopic"),
            &json!({"TopicArn": topic_arn("orders-created")}),
        )
        .await;
    assert_eq!((status, gone["code"].as_str()), (404, Some("NOT_FOUND")));
}
