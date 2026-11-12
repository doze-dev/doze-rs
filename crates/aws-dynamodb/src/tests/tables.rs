//! Table administration: create, describe, update, delete, list, time to live, point-in-time recovery, tags.

use serde_json::json;

use super::{World, message};

fn create(w: &mut World, name: &str) -> serde_json::Value {
    w.ok(
        "CreateTable",
        json!({"TableName": name, "BillingMode": "PAY_PER_REQUEST",
            "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
            "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}]}),
    )
}

#[test]
fn a_table_is_created_described_and_deleted_as_aws_shows_it() {
    let mut w = World::new();
    let made = create(&mut w, "orders");
    let d = &made["TableDescription"];
    assert_eq!(d["TableStatus"], "CREATING");
    assert_eq!(
        d["BillingModeSummary"],
        json!({"BillingMode": "PAY_PER_REQUEST"})
    );
    assert_eq!(
        d["KeySchema"],
        json!([{"AttributeName": "pk", "KeyType": "HASH"}])
    );
    assert_eq!(d["ProvisionedThroughput"]["ReadCapacityUnits"], 0);
    assert_eq!(
        d["TableArn"],
        "arn:aws:dynamodb:us-east-1:000000000000:table/orders"
    );
    assert!(d.get("WarmThroughput").is_none());
    let shown = w.ok("DescribeTable", json!({"TableName": "orders"}));
    let t = &shown["Table"];
    assert_eq!(t["TableStatus"], "ACTIVE");
    assert_eq!(
        t["WarmThroughput"],
        json!({"ReadUnitsPerSecond": 12000, "WriteUnitsPerSecond": 4000, "Status": "ACTIVE"})
    );
    assert!(
        t["BillingModeSummary"]
            .get("LastUpdateToPayPerRequestDateTime")
            .is_some()
    );
    assert_eq!(t["TableId"], d["TableId"]);
    assert_eq!(
        w.ok("DescribeTimeToLive", json!({"TableName": "orders"})),
        json!({"TimeToLiveDescription": {"TimeToLiveStatus": "DISABLED"}})
    );
    let gone = w.ok("DeleteTable", json!({"TableName": "orders"}));
    assert_eq!(gone["TableDescription"]["TableStatus"], "DELETING");
    assert!(gone["TableDescription"].get("KeySchema").is_none());
    let e = w.err("DescribeTable", json!({"TableName": "orders"}));
    assert_eq!(
        (e.code.as_ref(), message(&e)),
        (
            "ResourceNotFoundException",
            "Requested resource not found: Table: orders not found"
        )
    );
}

#[test]
fn the_refusals_that_were_recorded() {
    let mut w = World::new();
    create(&mut w, "orders");
    let e = w.err(
        "GetItem",
        json!({"TableName": "absent", "Key": {"pk": {"S": "a"}}}),
    );
    assert_eq!(
        (e.code.as_ref(), message(&e)),
        ("ResourceNotFoundException", "Requested resource not found")
    );
    let e = w.err(
        "CreateTable",
        json!({"TableName": "orders", "BillingMode": "PAY_PER_REQUEST",
            "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
            "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}]}),
    );
    assert_eq!(
        (e.code.as_ref(), message(&e)),
        ("ResourceInUseException", "Table already exists: orders")
    );
}

#[test]
fn schema_rules() {
    let mut w = World::new();
    let hash = json!({"AttributeName": "pk", "KeyType": "HASH"});
    let attr = |n: &str, t: &str| json!({"AttributeName": n, "AttributeType": t});
    let mut try_create = |schema: serde_json::Value,
                          attrs: serde_json::Value,
                          extra: serde_json::Value| {
        let mut req = json!({"TableName": "t1x", "BillingMode": "PAY_PER_REQUEST", "KeySchema": schema, "AttributeDefinitions": attrs});
        for (k, v) in extra.as_object().unwrap() {
            req[k] = v.clone();
        }
        w.err("CreateTable", req)
    };
    let e = try_create(
        json!([hash]),
        json!([attr("pk", "S"), attr("other", "S")]),
        json!({}),
    );
    assert!(message(&e).contains("Number of attributes in KeySchema does not exactly match"));
    let e = try_create(
        json!([{"AttributeName": "pk", "KeyType": "RANGE"}]),
        json!([attr("pk", "S")]),
        json!({}),
    );
    assert!(message(&e).contains("first KeySchemaElement is not a HASH"));
    let e = try_create(
        json!([hash]),
        json!([attr("pk", "S")]),
        json!({"BillingMode": "PAY_PER_REQUEST", "ProvisionedThroughput": {"ReadCapacityUnits": 1, "WriteCapacityUnits": 1}}),
    );
    assert!(
        message(&e).contains("Neither ReadCapacityUnits nor WriteCapacityUnits can be specified")
    );
    let e = try_create(
        json!([hash]),
        json!([attr("pk", "S")]),
        json!({"TableName": "a"}),
    );
    assert!(message(&e).contains("at 'tableName' failed to satisfy constraint: Member must have length greater than or equal to 3"));
    let e = try_create(
        json!([hash]),
        json!([attr("pk", "S")]),
        json!({"TableName": "bad name!"}),
    );
    assert!(message(&e).contains("regular expression pattern"));
    let e = try_create(
        json!([hash]),
        json!([attr("pk", "S")]),
        json!({"LocalSecondaryIndexes": [{"IndexName": "lsi", "KeySchema": [hash, {"AttributeName": "x", "KeyType": "RANGE"}], "Projection": {"ProjectionType": "ALL"}}]}),
    );
    assert!(message(&e).contains("does not have a range key"));
}

#[test]
fn listing_pages_by_name() {
    let mut w = World::new();
    for n in ["bravo", "alpha", "charlie"] {
        create(&mut w, n);
    }
    let page = w.ok("ListTables", json!({"Limit": 2}));
    assert_eq!(page["TableNames"], json!(["alpha", "bravo"]));
    assert_eq!(page["LastEvaluatedTableName"], "bravo");
    let rest = w.ok("ListTables", json!({"ExclusiveStartTableName": "bravo"}));
    assert_eq!(rest["TableNames"], json!(["charlie"]));
    assert!(rest.get("LastEvaluatedTableName").is_none());
}

#[test]
fn time_to_live_round_trips_and_follows_aws_rules() {
    let mut w = World::new();
    create(&mut w, "orders");
    let spec = json!({"Enabled": true, "AttributeName": "expires"});
    let on = w.ok(
        "UpdateTimeToLive",
        json!({"TableName": "orders", "TimeToLiveSpecification": spec}),
    );
    assert_eq!(on["TimeToLiveSpecification"], spec);
    let e = w.err(
        "UpdateTimeToLive",
        json!({"TableName": "orders", "TimeToLiveSpecification": spec}),
    );
    assert_eq!(message(&e), "TimeToLive is already enabled");
    assert_eq!(
        w.ok("DescribeTimeToLive", json!({"TableName": "orders"}))["TimeToLiveDescription"],
        json!({"TimeToLiveStatus": "ENABLED", "AttributeName": "expires"})
    );
    let off = json!({"TableName": "orders", "TimeToLiveSpecification": {"Enabled": false, "AttributeName": "expires"}});
    let e = w.err("UpdateTimeToLive", off.clone());
    assert!(message(&e).contains("modified multiple times"));
    w.clock.advance(3_600_001);
    w.ok("UpdateTimeToLive", off);
    assert_eq!(
        w.ok("DescribeTimeToLive", json!({"TableName": "orders"}))["TimeToLiveDescription"]["TimeToLiveStatus"],
        "DISABLED"
    );
}

#[test]
fn point_in_time_recovery_is_a_round_trip() {
    let mut w = World::new();
    create(&mut w, "orders");
    let before = w.ok("DescribeContinuousBackups", json!({"TableName": "orders"}));
    assert_eq!(
        before["ContinuousBackupsDescription"]["PointInTimeRecoveryDescription"],
        json!({"PointInTimeRecoveryStatus": "DISABLED"})
    );
    let on = w.ok(
        "UpdateContinuousBackups",
        json!({"TableName": "orders", "PointInTimeRecoverySpecification": {"PointInTimeRecoveryEnabled": true, "RecoveryPeriodInDays": 7}}),
    );
    let d = &on["ContinuousBackupsDescription"]["PointInTimeRecoveryDescription"];
    assert_eq!(
        (
            d["PointInTimeRecoveryStatus"].as_str(),
            d["RecoveryPeriodInDays"].as_i64()
        ),
        (Some("ENABLED"), Some(7))
    );
    assert!(
        d.get("EarliestRestorableDateTime").is_some()
            && d.get("LatestRestorableDateTime").is_some()
    );
    let again = w.ok("DescribeContinuousBackups", json!({"TableName": "orders"}));
    assert_eq!(
        again["ContinuousBackupsDescription"]["PointInTimeRecoveryDescription"]["PointInTimeRecoveryStatus"],
        "ENABLED"
    );
    let e = w.err("DescribeContinuousBackups", json!({"TableName": "absent"}));
    assert_eq!(e.code, "TableNotFoundException");
    w.ok("UpdateContinuousBackups", json!({"TableName": "orders", "PointInTimeRecoverySpecification": {"PointInTimeRecoveryEnabled": false}}));
    let off = w.ok("DescribeContinuousBackups", json!({"TableName": "orders"}));
    assert_eq!(
        off["ContinuousBackupsDescription"]["PointInTimeRecoveryDescription"],
        json!({"PointInTimeRecoveryStatus": "DISABLED"})
    );
}

#[test]
fn tags_round_trip_and_obey_limits() {
    let mut w = World::new();
    create(&mut w, "orders");
    let arn = "arn:aws:dynamodb:us-east-1:000000000000:table/orders";
    w.ok("TagResource", json!({"ResourceArn": arn, "Tags": [{"Key": "team", "Value": "a"}, {"Key": "env", "Value": "dev"}]}));
    assert_eq!(
        w.ok("ListTagsOfResource", json!({"ResourceArn": arn}))["Tags"],
        json!([{"Key": "env", "Value": "dev"}, {"Key": "team", "Value": "a"}])
    );
    w.ok(
        "UntagResource",
        json!({"ResourceArn": arn, "TagKeys": ["env"]}),
    );
    assert_eq!(
        w.ok("ListTagsOfResource", json!({"ResourceArn": arn}))["Tags"],
        json!([{"Key": "team", "Value": "a"}])
    );
    let e = w.err(
        "TagResource",
        json!({"ResourceArn": arn, "Tags": [{"Key": "aws:x", "Value": "a"}]}),
    );
    assert!(message(&e).contains("aws:"));
    let many: Vec<_> = (0..51)
        .map(|i| json!({"Key": format!("k{i}"), "Value": "v"}))
        .collect();
    assert_eq!(
        w.err("TagResource", json!({"ResourceArn": arn, "Tags": many}))
            .code,
        "LimitExceededException"
    );
    let e = w.err(
        "ListTagsOfResource",
        json!({"ResourceArn": "arn:aws:dynamodb:us-east-1:000000000000:table/absent"}),
    );
    assert_eq!(e.code, "ResourceNotFoundException");
}

#[test]
fn update_table_changes_billing_streams_and_indexes() {
    let mut w = World::new();
    create(&mut w, "orders");
    let e = w.err("UpdateTable", json!({"TableName": "orders"}));
    assert!(message(&e).starts_with("At least one of ProvisionedThroughput"));
    let up = w.ok(
        "UpdateTable",
        json!({"TableName": "orders", "BillingMode": "PROVISIONED", "ProvisionedThroughput": {"ReadCapacityUnits": 5, "WriteCapacityUnits": 2}}),
    );
    assert_eq!(up["TableDescription"]["TableStatus"], "UPDATING");
    let d = w.ok("DescribeTable", json!({"TableName": "orders"}));
    assert_eq!(d["Table"]["TableStatus"], "ACTIVE");
    assert_eq!(d["Table"]["ProvisionedThroughput"]["ReadCapacityUnits"], 5);
    assert!(d["Table"].get("BillingModeSummary").is_none());

    let on = w.ok("UpdateTable", json!({"TableName": "orders", "StreamSpecification": {"StreamEnabled": true, "StreamViewType": "NEW_AND_OLD_IMAGES"}}));
    let arn = on["TableDescription"]["LatestStreamArn"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(arn.contains(":table/orders/stream/"));
    let e = w.err("UpdateTable", json!({"TableName": "orders", "StreamSpecification": {"StreamEnabled": true, "StreamViewType": "KEYS_ONLY"}}));
    assert!(message(&e).starts_with("Table already has an enabled stream"));
    w.ok(
        "UpdateTable",
        json!({"TableName": "orders", "StreamSpecification": {"StreamEnabled": false}}),
    );
    assert!(
        w.ok("DescribeTable", json!({"TableName": "orders"}))["Table"]
            .get("LatestStreamArn")
            .is_none()
    );
    let e = w.err(
        "UpdateTable",
        json!({"TableName": "orders", "StreamSpecification": {"StreamEnabled": false}}),
    );
    assert!(message(&e).contains("does not have an enabled stream"));
    w.clock.advance(10);
    w.ok("UpdateTable", json!({"TableName": "orders", "StreamSpecification": {"StreamEnabled": true, "StreamViewType": "KEYS_ONLY"}}));
    let second = w.ok("DescribeTable", json!({"TableName": "orders"}));
    assert_ne!(second["Table"]["LatestStreamArn"], json!(arn));
}

#[test]
fn deletion_protection_blocks_delete() {
    let mut w = World::new();
    w.ok(
        "CreateTable",
        json!({"TableName": "keep", "BillingMode": "PAY_PER_REQUEST", "DeletionProtectionEnabled": true,
            "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
            "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}]}),
    );
    assert!(
        message(&w.err("DeleteTable", json!({"TableName": "keep"})))
            .contains("protected against deletion")
    );
    w.ok(
        "UpdateTable",
        json!({"TableName": "keep", "DeletionProtectionEnabled": false}),
    );
    w.ok("DeleteTable", json!({"TableName": "keep"}));
}

#[test]
fn transitional_states_last_as_long_as_the_window_says() {
    let mut w = World::new();
    w.core.as_mut().unwrap().windows.create_ms = 5_000;
    create(&mut w, "slow");
    assert_eq!(
        w.ok("DescribeTable", json!({"TableName": "slow"}))["Table"]["TableStatus"],
        "CREATING"
    );
    w.clock.advance(5_000);
    assert_eq!(
        w.ok("DescribeTable", json!({"TableName": "slow"}))["Table"]["TableStatus"],
        "ACTIVE"
    );
}
