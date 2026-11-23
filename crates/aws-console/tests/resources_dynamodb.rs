//! A table's pages beyond its items (indexes, settings, monitor, exports and streams) and the PartiQL editor, against a real
//! DynamoDB on a real bus: each call acts on the table the address names, the page's sentence reads the table, and the list
//! carries what the page draws its columns from.
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
use deep::ok;
use serde_json::{Value, json};
use world::{World, world};

const BASE: &str = "/api/v1/resources/dynamodb/tables/orders";

/// `orders` (pk, sk, one index) and `other` (pk), both on demand.
async fn tables(name: &str) -> World {
    let w = world(name, Mode::Soft, &[]);
    let defs = json!([{"AttributeName": "pk", "AttributeType": "S"}, {"AttributeName": "sk", "AttributeType": "S"},
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
    w
}

fn names(table: &Value, member: &str) -> Vec<String> {
    table["Table"][member]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|i| i["IndexName"].as_str().unwrap().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn an_index_is_made_and_dropped_on_this_table_and_no_other() {
    let w = tables("ddb-index").await;
    ok(
        &w,
        BASE,
        "UpdateTable",
        &json!({"TableName": "other",
            "AttributeDefinitions": [{"AttributeName": "status", "AttributeType": "S"}, {"AttributeName": "created_at", "AttributeType": "S"}],
            "GlobalSecondaryIndexUpdates": [{"Create": {"IndexName": "by-created",
                "KeySchema": [{"AttributeName": "status", "KeyType": "HASH"}, {"AttributeName": "created_at", "KeyType": "RANGE"}],
                "Projection": {"ProjectionType": "KEYS_ONLY"}}}]}),
    )
    .await;
    let described = ok(&w, BASE, "DescribeTable", &json!({})).await;
    assert_eq!(
        names(&described, "GlobalSecondaryIndexes"),
        ["by-status", "by-created"]
    );
    let other = w
        .app("dynamodb", "DescribeTable", &json!({"TableName": "other"}))
        .await;
    assert!(
        names(&other, "GlobalSecondaryIndexes").is_empty(),
        "an index reached another table"
    );
    ok(
        &w,
        BASE,
        "UpdateTable",
        &json!({"GlobalSecondaryIndexUpdates": [{"Delete": {"IndexName": "by-status"}}]}),
    )
    .await;
    let described = ok(&w, BASE, "DescribeTable", &json!({})).await;
    assert_eq!(names(&described, "GlobalSecondaryIndexes"), ["by-created"]);
}

#[tokio::test]
async fn capacity_protection_and_time_to_live_are_set_on_this_table() {
    let w = tables("ddb-settings").await;
    ok(
        &w,
        BASE,
        "UpdateTable",
        &json!({"BillingMode": "PROVISIONED", "ProvisionedThroughput": {"ReadCapacityUnits": 20, "WriteCapacityUnits": 10}}),
    )
    .await;
    ok(
        &w,
        BASE,
        "UpdateTable",
        &json!({"DeletionProtectionEnabled": true}),
    )
    .await;
    let t = ok(&w, BASE, "DescribeTable", &json!({})).await;
    assert_eq!(t["Table"]["ProvisionedThroughput"]["ReadCapacityUnits"], 20);
    assert_eq!(t["Table"]["DeletionProtectionEnabled"], true);
    // The page cannot name another table.
    ok(
        &w,
        BASE,
        "UpdateTimeToLive",
        &json!({"TableName": "other", "TimeToLiveSpecification": {"Enabled": true, "AttributeName": "expires_at"}}),
    )
    .await;
    let ttl = ok(&w, BASE, "DescribeTimeToLive", &json!({})).await;
    assert_eq!(ttl["TimeToLiveDescription"]["TimeToLiveStatus"], "ENABLED");
    assert_eq!(ttl["TimeToLiveDescription"]["AttributeName"], "expires_at");
    let other = w
        .app(
            "dynamodb",
            "DescribeTimeToLive",
            &json!({"TableName": "other"}),
        )
        .await;
    assert_eq!(
        other["TimeToLiveDescription"]["TimeToLiveStatus"],
        "DISABLED"
    );
    // A table under protection is not deleted.
    let (status, refused) = w.send("DELETE", BASE, &Value::Null).await;
    assert_eq!(status, 400, "{refused}");
}

#[tokio::test]
async fn contributor_insights_are_read_and_set_and_a_stream_is_turned_on() {
    let w = tables("ddb-monitor").await;
    let before = ok(&w, BASE, "DescribeContributorInsights", &json!({})).await;
    assert_eq!(before["ContributorInsightsStatus"], "DISABLED");
    ok(
        &w,
        BASE,
        "UpdateContributorInsights",
        &json!({"ContributorInsightsAction": "ENABLE", "IndexName": "by-status"}),
    )
    .await;
    let index = ok(
        &w,
        BASE,
        "DescribeContributorInsights",
        &json!({"IndexName": "by-status"}),
    )
    .await;
    assert_eq!(index["ContributorInsightsStatus"], "ENABLED");
    let table = ok(&w, BASE, "DescribeContributorInsights", &json!({})).await;
    assert_eq!(
        table["ContributorInsightsStatus"], "DISABLED",
        "the index's setting is its own"
    );
    ok(
        &w,
        BASE,
        "UpdateTable",
        &json!({"StreamSpecification": {"StreamEnabled": true, "StreamViewType": "NEW_AND_OLD_IMAGES"}}),
    )
    .await;
    let t = ok(&w, BASE, "DescribeTable", &json!({})).await;
    assert_eq!(
        t["Table"]["StreamSpecification"]["StreamViewType"],
        "NEW_AND_OLD_IMAGES"
    );
}

#[tokio::test]
async fn a_table_says_what_it_holds_and_its_list_carries_what_the_page_reads() {
    let w = tables("ddb-says").await;
    for i in 0..3 {
        w.app(
            "dynamodb",
            "PutItem",
            &json!({"TableName": "orders", "Item": {"pk": {"S": "a"}, "sk": {"S": i.to_string()}, "status": {"S": "new"}}}),
        )
        .await;
    }
    let (_, one) = w.get("/api/v1/resources/dynamodb/tables/orders").await;
    let said = one["sentence"].as_str().unwrap();
    assert!(
        said.starts_with("orders holds 3 items. One global secondary index, by-status, is Active. No DynamoDB stream is on."),
        "{said}"
    );
    assert_eq!(one["verbs"][0]["id"], "explore");
    let (_, other) = w.get("/api/v1/resources/dynamodb/tables/other").await;
    assert_eq!(
        other["sentence"],
        "other holds 0 items. It has no secondary index. No DynamoDB stream is on."
    );
    let (_, list) = w.get("/api/v1/resources/dynamodb/tables").await;
    let titles: Vec<&str> = list["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["title"].as_str().unwrap())
        .collect();
    let at = |t: &str| titles.iter().position(|x| *x == t).unwrap();
    let row = list["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "orders")
        .unwrap();
    let cell = |t: &str| row["cells"][at(t)].as_str().unwrap_or_default();
    assert!(cell("Attributes").contains("AttributeType"), "{row}");
    assert!(cell("Global indexes").contains("by-status"), "{row}");
    assert_eq!(cell("Capacity mode"), "PAY_PER_REQUEST");
    assert_eq!(cell("Items"), "3");
}
