//! A table's exports, its Kinesis destination and the PartiQL editor, against a real DynamoDB on a real bus: each call acts on the table
//! the address names, and a statement's refusal comes back as the service's.
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

#[tokio::test]
async fn a_table_is_exported_after_point_in_time_recovery_is_on_and_only_this_one() {
    let w = tables("ddb-export").await;
    w.app("s3", "CreateBucket", &json!({"Bucket": "larder-exports"}))
        .await;
    let (status, refused) = op(
        &w,
        BASE,
        "ExportTableToPointInTime",
        &json!({"S3Bucket": "larder-exports"}),
    )
    .await;
    assert_eq!(status, 400, "{refused}");
    assert_eq!(
        refused["code"], "PointInTimeRecoveryUnavailableException",
        "{refused}"
    );
    let off = ok(&w, BASE, "DescribeContinuousBackups", &json!({})).await;
    assert_eq!(
        off["ContinuousBackupsDescription"]["PointInTimeRecoveryDescription"]["PointInTimeRecoveryStatus"],
        "DISABLED"
    );
    ok(
        &w,
        BASE,
        "UpdateContinuousBackups",
        &json!({"PointInTimeRecoverySpecification": {"PointInTimeRecoveryEnabled": true}}),
    )
    .await;
    let made = ok(
        &w,
        BASE,
        "ExportTableToPointInTime",
        // The table in the request is not the page's: the page's table is what is exported.
        &json!({"TableArn": "arn:aws:dynamodb:us-east-1:000000000000:table/other", "S3Bucket": "larder-exports", "S3Prefix": "exports"}),
    )
    .await;
    assert!(
        made["ExportDescription"]["TableArn"]
            .as_str()
            .unwrap()
            .ends_with("table/orders"),
        "{made}"
    );
    let listed = ok(&w, BASE, "ListExports", &json!({})).await;
    assert_eq!(listed["ExportSummaries"].as_array().unwrap().len(), 1);
    let other = w
        .app(
            "dynamodb",
            "ListExports",
            &json!({"TableArn": "arn:aws:dynamodb:us-east-1:000000000000:table/other"}),
        )
        .await;
    assert_eq!(other["ExportSummaries"].as_array().map_or(0, Vec::len), 0);
}

#[tokio::test]
async fn a_partiql_statement_is_run_and_its_refusal_comes_back_as_the_services() {
    let w = tables("ddb-partiql").await;
    let path = "/api/v1/resources/dynamodb/tables/op/ExecuteStatement";
    let (status, put) = w
        .send(
            "POST",
            path,
            &json!({"Statement": "INSERT INTO \"orders\" VALUE {'pk': 'a', 'sk': '1', 'status': 'new'}"}),
        )
        .await;
    assert_eq!(status, 200, "{put}");
    let (status, got) = w
        .send(
            "POST",
            path,
            &json!({"Statement": "SELECT * FROM \"orders\" WHERE pk = 'a'"}),
        )
        .await;
    assert_eq!(status, 200, "{got}");
    assert_eq!(got["Items"][0]["status"]["S"], "new");
    let (status, refused) = w
        .send(
            "POST",
            path,
            &json!({"Statement": "UPDATE \"orders\" SET status = 'x' WHERE pk = 'zz' AND sk = '1'"}),
        )
        .await;
    assert_eq!(status, 400, "{refused}");
    assert_eq!(
        refused["code"], "ConditionalCheckFailedException",
        "{refused}"
    );
    // Other operations are not the editor's.
    let (status, _) = w
        .send(
            "POST",
            "/api/v1/resources/dynamodb/tables/op/DeleteTable",
            &json!({"TableName": "orders"}),
        )
        .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn a_kinesis_destination_is_turned_on_and_off_for_this_table_and_no_other() {
    let w = tables("ddb-kinesis").await;
    w.app(
        "kinesis",
        "CreateStream",
        &json!({"StreamName": "orders-changes", "ShardCount": 1}),
    )
    .await;
    let arn = "arn:aws:kinesis:us-east-1:000000000000:stream/orders-changes";
    // The page names another table; the table of the address is the one the destination is turned on for.
    let on = ok(
        &w,
        BASE,
        "EnableKinesisStreamingDestination",
        &json!({"TableName": "other", "StreamArn": arn}),
    )
    .await;
    assert_eq!(on["TableName"], "orders", "{on}");
    let seen = ok(&w, BASE, "DescribeKinesisStreamingDestination", &json!({})).await;
    assert_eq!(seen["KinesisDataStreamDestinations"][0]["StreamArn"], arn);
    let other = w
        .app(
            "dynamodb",
            "DescribeKinesisStreamingDestination",
            &json!({"TableName": "other"}),
        )
        .await;
    assert!(
        deep::none(&other["KinesisDataStreamDestinations"]),
        "{other}"
    );
    ok(
        &w,
        BASE,
        "DisableKinesisStreamingDestination",
        &json!({"StreamArn": arn}),
    )
    .await;
    let after = ok(&w, BASE, "DescribeKinesisStreamingDestination", &json!({})).await;
    assert_eq!(
        after["KinesisDataStreamDestinations"][0]["DestinationStatus"],
        "DISABLED"
    );
}
