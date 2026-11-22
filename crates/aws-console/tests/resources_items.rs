//! A table's items page against a real DynamoDB on a real bus: items put, scanned page by page, queried by the table's keys
//! and by an index, and deleted, each on the table the address names and no other.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

#[path = "resources/deep.rs"]
mod deep;
mod support;
// Each test binary uses some of the harness.
#[allow(dead_code)]
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use deep::{ok, op};
use serde_json::json;
use world::{World, world};

const BASE: &str = "/api/v1/resources/dynamodb/tables/orders";

/// `orders` (pk, sk; an index on status) with three items, put by the page, and an empty `other`.
async fn tables(name: &str) -> World {
    let w = world(name, Mode::Soft, &[]);
    let defs = json!([{"AttributeName": "pk", "AttributeType": "S"}, {"AttributeName": "sk", "AttributeType": "N"},
        {"AttributeName": "status", "AttributeType": "S"}]);
    let index = json!([{"IndexName": "by-status", "KeySchema": [{"AttributeName": "status", "KeyType": "HASH"}],
        "Projection": {"ProjectionType": "ALL"}}]);
    w.app("dynamodb", "CreateTable", &json!({"TableName": "orders", "BillingMode": "PAY_PER_REQUEST",
        "AttributeDefinitions": defs, "GlobalSecondaryIndexes": index,
        "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}, {"AttributeName": "sk", "KeyType": "RANGE"}]})).await;
    w.app(
        "dynamodb",
        "CreateTable",
        &json!({"TableName": "other", "BillingMode": "PAY_PER_REQUEST",
        "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}],
        "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}]}),
    )
    .await;
    for (pk, sk, status) in [("a", 1, "new"), ("a", 2, "shipped"), ("b", 1, "shipped")] {
        // The page cannot put into another table, whatever it names.
        let item = json!({"pk": {"S": pk}, "sk": {"N": sk.to_string()}, "status": {"S": status}});
        ok(
            &w,
            BASE,
            "PutItem",
            &json!({"TableName": "other", "Item": item}),
        )
        .await;
    }
    w
}

#[tokio::test]
async fn a_table_is_scanned_page_by_page_and_described() {
    let w = tables("deep-ddb-scan").await;
    let scan = ok(&w, BASE, "Scan", &json!({"Limit": 2})).await;
    assert_eq!(scan["Count"], 2);
    assert!(scan["LastEvaluatedKey"].is_object(), "{scan}");
    let rest = ok(
        &w,
        BASE,
        "Scan",
        &json!({"ExclusiveStartKey": scan["LastEvaluatedKey"]}),
    )
    .await;
    assert_eq!(rest["Count"], 1);
    let described = ok(&w, BASE, "DescribeTable", &json!({})).await;
    assert_eq!(
        described["Table"]["GlobalSecondaryIndexes"][0]["IndexName"],
        "by-status"
    );
    let other = w
        .app("dynamodb", "Scan", &json!({"TableName": "other"}))
        .await;
    assert_eq!(other["Count"], 0, "an item reached another table");
}

#[tokio::test]
async fn a_table_is_queried_by_its_keys_and_by_an_index_and_an_item_deleted() {
    let w = tables("deep-ddb-query").await;
    let q = ok(
        &w,
        BASE,
        "Query",
        &json!({"KeyConditionExpression": "#k = :k AND #s > :s",
        "ExpressionAttributeNames": {"#k": "pk", "#s": "sk"},
        "ExpressionAttributeValues": {":k": {"S": "a"}, ":s": {"N": "1"}}}),
    )
    .await;
    assert_eq!(
        (q["Count"].as_u64(), q["Items"][0]["status"]["S"].as_str()),
        (Some(1), Some("shipped"))
    );
    let by = ok(&w, BASE, "Query", &json!({"IndexName": "by-status", "KeyConditionExpression": "#k = :k",
        "ExpressionAttributeNames": {"#k": "status"}, "ExpressionAttributeValues": {":k": {"S": "shipped"}}})).await;
    assert_eq!(by["Count"], 2);
    ok(
        &w,
        BASE,
        "DeleteItem",
        &json!({"Key": {"pk": {"S": "a"}, "sk": {"N": "1"}}}),
    )
    .await;
    assert_eq!(ok(&w, BASE, "Scan", &json!({})).await["Count"], 2);
    let (status, bad) = op(
        &w,
        BASE,
        "PutItem",
        &json!({"Item": {"status": {"S": "x"}}}),
    )
    .await;
    assert_eq!(
        (status, bad["code"].as_str()),
        (400, Some("ValidationException")),
        "{bad}"
    );
}
