//! Resource policies: stored with revisions, and weighed by IAM with every request on the table, its indexes and its stream.

use serde_json::json;

use super::bus::Bus;

const TABLE: &str = "arn:aws:dynamodb:us-east-1:000000000000:table/tab";

fn deny_reads() -> String {
    json!({"Version": "2012-10-17", "Statement": [{
        "Effect": "Deny", "Principal": "*", "Action": ["dynamodb:GetItem", "dynamodb:Query"],
        "Resource": [TABLE, format!("{TABLE}/index/*")]}]})
    .to_string()
}

#[tokio::test]
async fn a_policy_is_kept_with_a_revision_and_conditional_puts_hold_to_it() {
    let bus = Bus::new();
    bus.table("tab").await;
    let e = bus
        .call("GetResourcePolicy", json!({"ResourceArn": TABLE}))
        .await
        .unwrap_err();
    assert_eq!(e.code, "PolicyNotFoundException");
    let first = bus
        .ok("PutResourcePolicy", json!({"ResourceArn": TABLE, "Policy": deny_reads(), "ExpectedRevisionId": "NO_POLICY"}))
        .await["RevisionId"]
        .clone();
    // The same document again keeps its revision.
    let again = bus
        .ok(
            "PutResourcePolicy",
            json!({"ResourceArn": TABLE, "Policy": deny_reads()}),
        )
        .await;
    assert_eq!(again["RevisionId"], first);
    let got = bus
        .ok("GetResourcePolicy", json!({"ResourceArn": TABLE}))
        .await;
    assert_eq!(
        (got["Policy"].as_str(), &got["RevisionId"]),
        (Some(deny_reads().as_str()), &first)
    );
    let e = bus
        .call("PutResourcePolicy", json!({"ResourceArn": TABLE, "Policy": deny_reads(), "ExpectedRevisionId": "NO_POLICY"}))
        .await
        .unwrap_err();
    assert_eq!(e.code, "PolicyNotFoundException");
    let e = bus
        .call(
            "DeleteResourcePolicy",
            json!({"ResourceArn": TABLE, "ExpectedRevisionId": "1"}),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code, "PolicyNotFoundException");
    let gone = bus
        .ok(
            "DeleteResourcePolicy",
            json!({"ResourceArn": TABLE, "ExpectedRevisionId": first}),
        )
        .await;
    assert_eq!(gone["RevisionId"], first);
    // Deleting again is not an error.
    assert_eq!(
        bus.ok("DeleteResourcePolicy", json!({"ResourceArn": TABLE}))
            .await,
        json!({})
    );
}

#[tokio::test]
async fn a_policy_is_refused_on_what_is_not_a_table_and_when_it_is_not_one() {
    let bus = Bus::new();
    bus.table("tab").await;
    for (arn, code) in [
        (
            "arn:aws:dynamodb:us-east-1:000000000000:table/none",
            "ResourceNotFoundException",
        ),
        ("tab", "ValidationException"),
    ] {
        let e = bus
            .call(
                "PutResourcePolicy",
                json!({"ResourceArn": arn, "Policy": deny_reads()}),
            )
            .await
            .unwrap_err();
        assert_eq!(e.code, code, "{arn}");
    }
    let e = bus
        .call(
            "PutResourcePolicy",
            json!({"ResourceArn": TABLE, "Policy": "{not json"}),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code, "ValidationException");
}

#[tokio::test]
async fn iam_refuses_what_the_policy_denies_on_the_table_and_its_indexes() {
    let bus = Bus::new();
    bus.ok(
        "CreateTable",
        json!({"TableName": "tab", "BillingMode": "PAY_PER_REQUEST", "ResourcePolicy": deny_reads(),
            "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
            "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}, {"AttributeName": "v", "AttributeType": "S"}],
            "GlobalSecondaryIndexes": [{"IndexName": "byv", "KeySchema": [{"AttributeName": "v", "KeyType": "HASH"}], "Projection": {"ProjectionType": "ALL"}}]}),
    )
    .await;
    bus.enforce_iam().await;
    bus.ok(
        "PutItem",
        json!({"TableName": "tab", "Item": {"pk": {"S": "a"}, "v": {"S": "1"}}}),
    )
    .await;
    let e = bus
        .call(
            "GetItem",
            json!({"TableName": "tab", "Key": {"pk": {"S": "a"}}}),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code, "AccessDeniedException", "{}", e.message);
    assert!(e.message.contains("dynamodb:GetItem"), "{}", e.message);
    let e = bus
        .call(
            "Query",
            json!({"TableName": "tab", "IndexName": "byv", "KeyConditionExpression": "v = :v", "ExpressionAttributeValues": {":v": {"S": "1"}}}),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code, "AccessDeniedException");
    // Without the policy the same caller reads.
    bus.ok("DeleteResourcePolicy", json!({"ResourceArn": TABLE}))
        .await;
    let got = bus
        .ok(
            "GetItem",
            json!({"TableName": "tab", "Key": {"pk": {"S": "a"}}}),
        )
        .await;
    assert_eq!(got["Item"]["v"], json!({"S": "1"}));
    // A table's policies go with it.
    bus.ok(
        "PutResourcePolicy",
        json!({"ResourceArn": TABLE, "Policy": deny_reads()}),
    )
    .await;
    bus.ok("DeleteTable", json!({"TableName": "tab"})).await;
    bus.table("tab").await;
    bus.ok(
        "PutItem",
        json!({"TableName": "tab", "Item": {"pk": {"S": "a"}}}),
    )
    .await;
    bus.ok(
        "GetItem",
        json!({"TableName": "tab", "Key": {"pk": {"S": "a"}}}),
    )
    .await;
}
