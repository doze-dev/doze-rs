//! The operations of a kind that no resource is bound to (`POST …/<kind>/op/<Operation>`): a create with a form of its own,
//! made as the console against real services; and the 404 for a kind the console does not describe.
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
use serde_json::json;
use world::world;

const BASE: &str = "/api/v1/resources";

#[tokio::test]
async fn a_queue_is_created_with_its_own_attributes_and_only_by_the_operations_a_kind_lists() {
    let w = world("ops-sqs", Mode::Enforce, &[]);
    let (status, made) = w
        .send(
            "POST",
            &format!("{BASE}/sqs/queues/op/CreateQueue"),
            &json!({"QueueName": "orders.fifo", "Attributes": {"FifoQueue": "true", "ContentBasedDeduplication": "true"},
                "tags": {"env": "dev"}}),
        )
        .await;
    assert_eq!(status, 200, "{made}");
    assert!(made["QueueUrl"].as_str().unwrap().ends_with("/orders.fifo"));
    let (_, detail) = w.get(&format!("{BASE}/sqs/queues/orders.fifo")).await;
    assert_eq!(
        world::field(&detail, "Content-based deduplication")["value"],
        "Enabled"
    );
    // The call is the console's, and shows in Live as its own.
    assert!(
        w.live().iter().any(|l| l == "console sqs CreateQueue"),
        "{:?}",
        w.live()
    );
    // An operation the kind does not list is not made, whatever it names.
    let (status, refused) = w
        .send(
            "POST",
            &format!("{BASE}/sqs/queues/op/DeleteQueue"),
            &json!({"QueueUrl": "http://127.0.0.1:4566/000000000000/orders.fifo"}),
        )
        .await;
    assert_eq!(
        (status, refused["code"].as_str()),
        (404, Some("NOT_IN_PAGE"))
    );
    // The service's own refusal passes through with its status, code and words.
    let (status, again) = w
        .send(
            "POST",
            &format!("{BASE}/sqs/queues/op/CreateQueue"),
            &json!({"QueueName": "bad name!"}),
        )
        .await;
    assert_eq!(status, 400, "{again}");
    assert_ne!(again["code"], "NOT_IN_PAGE");
    // A body that is not an object is refused before any call.
    let (status, bad) = w
        .send(
            "POST",
            &format!("{BASE}/sqs/queues/op/CreateQueue"),
            &json!([1]),
        )
        .await;
    assert_eq!((status, bad["code"].as_str()), (400, Some("BAD_REQUEST")));
}

#[tokio::test]
async fn the_first_slices_creates_are_kind_operations() {
    let w = world("ops-first", Mode::Enforce, &[]);
    let (status, topic) = w
        .send(
            "POST",
            &format!("{BASE}/sns/topics/op/CreateTopic"),
            &json!({"Name": "order-events", "Attributes": {"DisplayName": "orders"}}),
        )
        .await;
    assert_eq!(status, 200, "{topic}");
    let arn = topic["TopicArn"].as_str().unwrap().to_owned();
    w.app("sqs", "CreateQueue", &json!({"QueueName": "inbox"}))
        .await;
    let (status, sub) = w
        .send(
            "POST",
            &format!("{BASE}/sns/subscriptions/op/Subscribe"),
            &json!({"TopicArn": arn, "Protocol": "sqs",
                "Endpoint": "arn:aws:sqs:us-east-1:000000000000:inbox", "ReturnSubscriptionArn": true}),
        )
        .await;
    assert_eq!(status, 200, "{sub}");
    assert!(sub["SubscriptionArn"].as_str().unwrap().starts_with(&arn));
    let (status, bucket) = w
        .send(
            "POST",
            &format!("{BASE}/s3/buckets/op/CreateBucket"),
            &json!({"Bucket": "product-images"}),
        )
        .await;
    assert_eq!(status, 200, "{bucket}");
    let (status, table) = w
        .send(
            "POST",
            &format!("{BASE}/dynamodb/tables/op/CreateTable"),
            &json!({"TableName": "orders", "BillingMode": "PAY_PER_REQUEST",
                "AttributeDefinitions": [{"AttributeName": "id", "AttributeType": "S"}],
                "KeySchema": [{"AttributeName": "id", "KeyType": "HASH"}]}),
        )
        .await;
    assert_eq!(status, 200, "{table}");
    for (path, name) in [
        ("sns/topics", "order-events"),
        ("s3/buckets", "product-images"),
        ("dynamodb/tables", "orders"),
    ] {
        let (status, one) = w.get(&format!("{BASE}/{path}/{name}")).await;
        assert_eq!(status, 200, "{path}: {one}");
    }
}

#[tokio::test]
async fn an_unknown_kind_is_a_kind_that_is_not_there_and_not_a_resource() {
    let w = world("ops-kind", Mode::Soft, &[]);
    for path in [
        "cloudwatch/dashboards",
        "cloudwatch/dashboards/main",
        "sqs/nothing",
    ] {
        let (status, body) = w.get(&format!("{BASE}/{path}")).await;
        assert_eq!(
            (status, body["code"].as_str()),
            (404, Some("NO_SUCH_KIND")),
            "{path}: {body}"
        );
    }
    let (status, body) = w
        .send(
            "POST",
            &format!("{BASE}/cloudwatch/dashboards/op/GetDashboard"),
            &json!({}),
        )
        .await;
    assert_eq!((status, body["code"].as_str()), (404, Some("NO_SUCH_KIND")));
    // A service the console has no descriptor for is still NOT_FOUND.
    let (_, body) = w.get(&format!("{BASE}/nothing/things")).await;
    assert_eq!(body["code"], "NOT_FOUND");
}

#[tokio::test]
async fn a_resource_named_op_is_still_a_resource() {
    let w = world("ops-named-op", Mode::Soft, &[]);
    w.app("sqs", "CreateQueue", &json!({"QueueName": "op"}))
        .await;
    let (status, detail) = w.get(&format!("{BASE}/sqs/queues/op")).await;
    assert_eq!(status, 200, "{detail}");
    // Its sub-addresses (`…/op/tags`) are the kind's operation address: a known limit of the contract's addresses, for a name
    // that is the word `op` alone.
    let (res, _) = support::send(&w.app, support::get(&format!("{BASE}/sqs/queues/op/tags"))).await;
    assert_eq!(res.status(), 405);
}
