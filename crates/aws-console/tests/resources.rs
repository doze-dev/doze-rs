//! The Resources API against real services on a real bus: each descriptor's create, list, detail, reveal and delete, made as
//! the caller `console`, seen in the Live record, and allowed under `--iam enforce`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use axum::http::header;
use serde_json::{Value, json};
use support::{json as answer, keys, ts_keys};
use world::{encode, field, world, write};

#[tokio::test]
async fn a_queue_is_created_listed_opened_and_deleted_as_the_console_and_live_shows_it() {
    let w = world("sqs", Mode::Enforce, &[]);
    let base = "/api/v1/resources/sqs/queues";
    let dlq = w
        .round_trip(base, &json!({"QueueName": "orders-dlq"}), "orders-dlq")
        .await;
    assert_eq!(dlq, "orders-dlq");
    let form = json!({"QueueName": "orders", "VisibilityTimeout": "45"});
    w.round_trip(base, &form, "orders").await;
    let (_, list) = w.get(base).await;
    assert_eq!(list["operation"], "ListQueues");
    let titles: Vec<&str> = list["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["title"].as_str().unwrap())
        .collect();
    assert_eq!(
        titles,
        [
            "Type",
            "Messages available",
            "In flight",
            "Dead-letter queue",
            "Created"
        ]
    );
    let (_, detail) = w.get(&format!("{base}/orders")).await;
    assert_eq!(field(&detail, "Visibility timeout")["value"], "45");
    assert_eq!(field(&detail, "Type")["value"], "Standard");
    assert!(
        field(&detail, "URL")["value"]
            .as_str()
            .unwrap()
            .ends_with("/000000000000/orders")
    );
    assert_eq!(
        detail["operations"],
        json!([
            "GetQueueUrl",
            "GetQueueAttributes",
            "ListEventSourceMappings",
            "ListDeadLetterSourceQueues"
        ])
    );
    // The detail's recent calls are the record's calls that acted on this queue.
    let calls = detail["calls"].as_array().unwrap();
    assert!(!calls.is_empty());
    assert!(calls.iter().all(|c| c["caller"] == "console"), "{calls:?}");
    // A missing queue is the service's own refusal.
    let (status, missing) = w.get(&format!("{base}/nope")).await;
    assert_eq!(
        (status, missing["code"].as_str()),
        (400, Some("QueueDoesNotExist")),
        "{missing}"
    );
    w.delete(base, "orders").await;
    let live = w.live();
    for want in [
        "console sqs CreateQueue",
        "console sqs ListQueues",
        "console sqs GetQueueAttributes",
        "console sqs DeleteQueue",
    ] {
        assert!(live.iter().any(|l| l == want), "{want} not in {live:?}");
    }
    assert!(live.iter().all(|l| l.starts_with("console ")), "{live:?}");
}

#[tokio::test]
async fn a_topic_and_its_subscription_link_to_each_other_and_to_the_queue() {
    let w = world("sns", Mode::Soft, &[]);
    w.send(
        "POST",
        "/api/v1/resources/sqs/queues",
        &json!({"QueueName": "inbox"}),
    )
    .await;
    let topic = w
        .round_trip(
            "/api/v1/resources/sns/topics",
            &json!({"Name": "orders"}),
            "orders",
        )
        .await;
    assert_eq!(topic, "orders");
    let arn = "arn:aws:sns:us-east-1:000000000000:orders";
    let form = json!({"TopicArn": arn, "Protocol": "sqs", "Endpoint": "arn:aws:sqs:us-east-1:000000000000:inbox"});
    let sub = w
        .round_trip("/api/v1/resources/sns/subscriptions", &form, "")
        .await;
    assert!(sub.starts_with(&format!("{arn}:")), "{sub}");
    let (_, detail) = w
        .get(&format!(
            "/api/v1/resources/sns/subscriptions/{}",
            encode(&sub)
        ))
        .await;
    assert_eq!(
        field(&detail, "Endpoint")["link"],
        json!({"service": "sqs", "kind": "queues", "name": "inbox"})
    );
    assert_eq!(
        field(&detail, "Topic")["link"],
        json!({"service": "sns", "kind": "topics", "name": "orders"})
    );
    let (_, topic) = w.get("/api/v1/resources/sns/topics/orders").await;
    let subs = &topic["subs"][0];
    assert_eq!(subs["title"], "Subscriptions");
    assert_eq!(subs["rows"][0]["link"]["name"], sub.as_str());
    w.delete("/api/v1/resources/sns/subscriptions", &sub).await;
    w.delete("/api/v1/resources/sns/topics", "orders").await;
}

#[tokio::test]
async fn a_bucket_shows_its_objects_and_a_table_its_keys() {
    let w = world("data", Mode::Enforce, &[]);
    w.round_trip(
        "/api/v1/resources/s3/buckets",
        &json!({"Bucket": "photos"}),
        "photos",
    )
    .await;
    let (_, detail) = w.get("/api/v1/resources/s3/buckets/photos").await;
    assert_eq!(detail["subs"][0]["title"], "Objects");
    assert_eq!(detail["subs"][0]["rows"], json!([]));
    w.delete("/api/v1/resources/s3/buckets", "photos").await;
    let form = json!({"TableName": "orders", "PartitionKey": "id", "PartitionKeyType": "S", "SortKey": "", "SortKeyType": "S"});
    w.round_trip("/api/v1/resources/dynamodb/tables", &form, "orders")
        .await;
    let (_, detail) = w.get("/api/v1/resources/dynamodb/tables/orders").await;
    assert_eq!(field(&detail, "Partition key")["value"], "id");
    assert_eq!(field(&detail, "Sort key")["value"], Value::Null);
    let form = json!({"TableName": "events", "PartitionKey": "pk", "PartitionKeyType": "S", "SortKey": "at", "SortKeyType": "N"});
    w.round_trip("/api/v1/resources/dynamodb/tables", &form, "events")
        .await;
    let (_, detail) = w.get("/api/v1/resources/dynamodb/tables/events").await;
    assert_eq!(field(&detail, "Sort key")["value"], "at");
    w.delete("/api/v1/resources/dynamodb/tables", "events")
        .await;
    // A select that is not one of its choices is the console's own refusal, before any call.
    let bad = json!({"TableName": "x", "PartitionKey": "id", "PartitionKeyType": "Q"});
    let (status, refused) = w
        .send("POST", "/api/v1/resources/dynamodb/tables", &bad)
        .await;
    assert_eq!(
        (status, refused["code"].as_str()),
        (400, Some("BAD_REQUEST")),
        "{refused}"
    );
}

#[tokio::test]
async fn a_secret_and_a_secure_parameter_stay_hidden_until_revealed() {
    let w = world("secrets", Mode::Enforce, &[]);
    let form = json!({"Name": "larder/prod/stripe", "SecretString": "sk_live_1", "Description": "the key"});
    let name = w
        .round_trip(
            "/api/v1/resources/secretsmanager/secrets",
            &form,
            "larder/prod/stripe",
        )
        .await;
    let one = format!("/api/v1/resources/secretsmanager/secrets/{}", encode(&name));
    let (_, detail) = w.get(&one).await;
    assert!(!detail.to_string().contains("sk_live_1"), "{detail}");
    assert_eq!(field(&detail, "Value")["hidden"], true);
    assert_eq!(detail["reveal"], "Reveal value");
    let before = w.live().len();
    let (status, shown) = w.send("POST", &format!("{one}/reveal"), &json!({})).await;
    assert_eq!(status, 200, "{shown}");
    assert_eq!(
        shown,
        json!({"value": "sk_live_1", "operation": "GetSecretValue"})
    );
    // Secrets Manager decrypts the value with KMS for the console: that call is the console's too.
    assert_eq!(
        w.live()[before..],
        [
            "console kms Decrypt",
            "console secretsmanager GetSecretValue"
        ]
    );
    w.delete("/api/v1/resources/secretsmanager/secrets", &name)
        .await;

    let form = json!({"Name": "/larder/db", "Type": "SecureString", "Value": "hunter2"});
    let name = w
        .round_trip("/api/v1/resources/ssm/parameters", &form, "/larder/db")
        .await;
    let one = format!("/api/v1/resources/ssm/parameters/{}", encode(&name));
    let (_, detail) = w.get(&one).await;
    assert!(!detail.to_string().contains("hunter2"), "{detail}");
    assert_eq!(field(&detail, "Value")["hidden"], true);
    let (_, shown) = w.send("POST", &format!("{one}/reveal"), &json!({})).await;
    assert_eq!(shown["value"], "hunter2");
    let form = json!({"Name": "plain", "Type": "String", "Value": "visible"});
    w.round_trip("/api/v1/resources/ssm/parameters", &form, "plain")
        .await;
    let (_, detail) = w.get("/api/v1/resources/ssm/parameters/plain").await;
    assert_eq!(field(&detail, "Value")["value"], "visible");
    w.delete("/api/v1/resources/ssm/parameters", "/larder/db")
        .await;
}

#[tokio::test]
async fn a_key_is_made_and_scheduled_for_deletion_and_functions_list_empty() {
    let w = world("kms", Mode::Enforce, &[]);
    let form = json!({"Description": "orders", "KeySpec": "SYMMETRIC_DEFAULT", "KeyUsage": "ENCRYPT_DECRYPT"});
    let id = w.round_trip("/api/v1/resources/kms/keys", &form, "").await;
    let (_, detail) = w.get(&format!("/api/v1/resources/kms/keys/{id}")).await;
    assert_eq!(field(&detail, "State")["value"], "Enabled");
    let (status, _) = w
        .send(
            "DELETE",
            &format!("/api/v1/resources/kms/keys/{id}"),
            &json!({}),
        )
        .await;
    assert_eq!(status, 200);
    let (_, detail) = w.get(&format!("/api/v1/resources/kms/keys/{id}")).await;
    assert_eq!(field(&detail, "State")["value"], "PendingDeletion");
    let (status, list) = w.get("/api/v1/resources/lambda/functions").await;
    assert_eq!((status, &list["rows"]), (200, &json!([])), "{list}");
    assert_eq!(list["create"], Value::Null);
}

#[tokio::test]
async fn the_index_counts_what_is_served_and_says_what_is_not_described() {
    let w = world("index", Mode::Soft, &["sqs", "kms"]);
    w.send(
        "POST",
        "/api/v1/resources/sqs/queues",
        &json!({"QueueName": "a"}),
    )
    .await;
    w.send(
        "POST",
        "/api/v1/resources/sqs/queues",
        &json!({"QueueName": "b"}),
    )
    .await;
    let (status, index) = w.get("/api/v1/resources").await;
    assert_eq!(status, 200, "{index}");
    assert_eq!(keys(&index), ts_keys("ResourcesIndex"));
    let svc = |n: &str| {
        index["services"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == n)
            .unwrap()
            .clone()
    };
    let sqs = svc("sqs");
    assert_eq!(keys(&sqs), ts_keys("ResourceService"));
    assert_eq!(
        (&sqs["group"], &sqs["served"], &sqs["described"]),
        (&json!("Messaging and events"), &json!(true), &json!(true))
    );
    assert_eq!(keys(&sqs["kinds"][0]), ts_keys("KindCount"));
    assert_eq!(
        (&sqs["kinds"][0]["count"], &sqs["kinds"][0]["names"]),
        (&json!(2), &json!(["a", "b"]))
    );
    // A service this run does not serve is not woken to be counted, and its pages say why.
    let sns = svc("sns");
    assert_eq!(
        (&sns["served"], &sns["kinds"][0]["count"]),
        (&json!(false), &Value::Null)
    );
    let (status, refused) = w.get("/api/v1/resources/sns/topics").await;
    assert_eq!(
        (status, refused["code"].as_str()),
        (409, Some("NOT_SERVED"))
    );
    assert_eq!(svc("iam")["described"], false);
    // Without counts nothing is called.
    let before = w.live().len();
    let (_, bare) = w.get("/api/v1/resources?counts=no").await;
    assert_eq!(w.live().len(), before);
    assert_eq!(bare["services"][0]["kinds"][0]["count"], Value::Null);
    assert!(
        svc("sts")["note"]
            .as_str()
            .unwrap()
            .contains("no resources")
    );
    // Writes need the same-origin proof.
    let mut req = write(
        "POST",
        "/api/v1/resources/sqs/queues",
        &json!({"QueueName": "c"}),
    );
    req.headers_mut().remove(header::ORIGIN);
    let (status, _) = answer(&w.app, req).await;
    assert_eq!(status, 403);
}

#[tokio::test]
async fn under_iam_enforce_a_user_with_no_policy_is_refused_where_the_console_is_allowed() {
    let w = world("enforce", Mode::Enforce, &[]);
    let peers = w.state.facts.peers.clone().unwrap();
    let root = aws_wire::service::Caller {
        access_key_id: "test".to_owned(),
        account_id: "000000000000".to_owned(),
        region: "us-east-1".to_owned(),
        endpoint: "http://127.0.0.1:4566".to_owned(),
        service: None,
        session_token: None,
    };
    peers
        .call_as(&root, "iam", "CreateUser", &json!({"UserName": "dev"}))
        .await
        .unwrap();
    let key = peers
        .call_as(&root, "iam", "CreateAccessKey", &json!({"UserName": "dev"}))
        .await
        .unwrap();
    let dev = aws_wire::service::Caller {
        access_key_id: key["AccessKey"]["AccessKeyId"].as_str().unwrap().to_owned(),
        ..root
    };
    let refused = peers
        .call_as(&dev, "sqs", "ListQueues", &json!({}))
        .await
        .unwrap_err();
    assert_eq!(refused.code, "AccessDeniedException", "{refused:?}");
    let (status, list) = w.get("/api/v1/resources/sqs/queues").await;
    assert_eq!(status, 200, "{list}");
}
