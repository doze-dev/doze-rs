//! On-demand backups and restores, and point-in-time recovery over the recovery log.

use serde_json::{Value, json};

use super::{World, message};

const START_S: f64 = 1_700_000_000.0;

fn items(w: &mut World, table: &str) -> Vec<String> {
    let out = w.ok("Scan", json!({"TableName": table}));
    let mut v: Vec<String> = out["Items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            format!(
                "{}={}",
                i["pk"]["S"].as_str().unwrap(),
                i.get("v").and_then(|v| v["S"].as_str()).unwrap_or("-")
            )
        })
        .collect();
    v.sort();
    v
}

fn with_index(w: &mut World) {
    w.ok(
        "CreateTable",
        json!({"TableName": "src", "BillingMode": "PAY_PER_REQUEST",
            "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
            "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}, {"AttributeName": "v", "AttributeType": "S"}],
            "GlobalSecondaryIndexes": [{"IndexName": "byv", "KeySchema": [{"AttributeName": "v", "KeyType": "HASH"}], "Projection": {"ProjectionType": "ALL"}}],
            "StreamSpecification": {"StreamEnabled": true, "StreamViewType": "NEW_IMAGE"},
            "Tags": [{"Key": "team", "Value": "x"}]}),
    );
}

#[test]
fn a_backup_is_a_copy_that_outlives_its_table_and_restores_with_its_indexes() {
    let mut w = World::new();
    with_index(&mut w);
    w.put("src", json!({"pk": {"S": "a"}, "v": {"S": "1"}}));
    w.put("src", json!({"pk": {"S": "b"}, "v": {"S": "2"}}));
    let made = w.ok("CreateBackup", json!({"TableName": "src", "BackupName": "first"}));
    let details = &made["BackupDetails"];
    assert_eq!(details["BackupStatus"], "CREATING");
    assert_eq!(details["BackupType"], "USER");
    let arn = details["BackupArn"].as_str().unwrap().to_owned();
    assert!(arn.starts_with("arn:aws:dynamodb:us-east-1:000000000000:table/src/backup/"), "{arn}");
    // Later writes are not in it.
    w.put("src", json!({"pk": {"S": "a"}, "v": {"S": "changed"}}));
    w.put("src", json!({"pk": {"S": "c"}, "v": {"S": "3"}}));
    let shown = w.ok("DescribeBackup", json!({"BackupArn": arn}))["BackupDescription"].clone();
    assert_eq!(shown["BackupDetails"]["BackupStatus"], "AVAILABLE");
    assert_eq!(shown["SourceTableDetails"]["ItemCount"], 2);
    assert_eq!(shown["SourceTableDetails"]["BillingMode"], "PAY_PER_REQUEST");
    assert_eq!(shown["SourceTableFeatureDetails"]["GlobalSecondaryIndexes"][0]["IndexName"], "byv");
    assert_eq!(shown["SourceTableFeatureDetails"]["StreamDescription"]["StreamViewType"], "NEW_IMAGE");
    w.ok("DeleteTable", json!({"TableName": "src"}));
    restored_from(&mut w, &arn);
}

/// The table a backup of `src` restores to, its index and what it does not bring; then the backup listed and deleted.
fn restored_from(w: &mut World, arn: &str) {
    let restored = w.ok("RestoreTableFromBackup", json!({"TargetTableName": "back", "BackupArn": arn}));
    let d = &restored["TableDescription"];
    assert_eq!(d["TableStatus"], "CREATING");
    assert_eq!(d["RestoreSummary"]["SourceBackupArn"], arn);
    assert_eq!(d["RestoreSummary"]["RestoreInProgress"], true);
    assert_eq!(items(w, "back"), ["a=1", "b=2"]);
    // The index came along and holds the items; the stream and tags did not.
    let by_v = w.ok(
        "Query",
        json!({"TableName": "back", "IndexName": "byv", "KeyConditionExpression": "v = :v", "ExpressionAttributeValues": {":v": {"S": "2"}}}),
    );
    assert_eq!(by_v["Count"], 1);
    let t = w.ok("DescribeTable", json!({"TableName": "back"}))["Table"].clone();
    assert!(t.get("StreamSpecification").is_none());
    assert_eq!(t["RestoreSummary"]["RestoreInProgress"], false);
    assert_eq!(w.ok("ListTagsOfResource", json!({"ResourceArn": t["TableArn"]}))["Tags"], json!([]));
    let again = w.err("RestoreTableFromBackup", json!({"TargetTableName": "back", "BackupArn": arn}));
    assert_eq!(again.code, "TableAlreadyExistsException");
    let listed = w.ok("ListBackups", json!({}));
    assert_eq!(listed["BackupSummaries"].as_array().unwrap().len(), 1);
    assert_eq!(listed["BackupSummaries"][0]["TableName"], "src");
    let gone = w.ok("DeleteBackup", json!({"BackupArn": arn}));
    assert_eq!(gone["BackupDescription"]["BackupDetails"]["BackupStatus"], "DELETED");
    let e = w.err("DescribeBackup", json!({"BackupArn": arn}));
    assert_eq!(e.code, "BackupNotFoundException");
    assert_eq!(w.ok("ListBackups", json!({}))["BackupSummaries"], json!([]));
}

#[test]
fn overrides_leave_out_indexes_and_change_the_billing_mode() {
    let mut w = World::new();
    with_index(&mut w);
    w.put("src", json!({"pk": {"S": "a"}, "v": {"S": "1"}}));
    let arn = w.ok("CreateBackup", json!({"TableName": "src", "BackupName": "bkp"}))["BackupDetails"]["BackupArn"].clone();
    let out = w.ok(
        "RestoreTableFromBackup",
        json!({"TargetTableName": "lean", "BackupArn": arn, "GlobalSecondaryIndexOverride": [],
            "BillingModeOverride": "PROVISIONED", "ProvisionedThroughputOverride": {"ReadCapacityUnits": 3, "WriteCapacityUnits": 4}}),
    );
    let d = &out["TableDescription"];
    assert!(d.get("GlobalSecondaryIndexes").is_none());
    assert_eq!(d["ProvisionedThroughput"]["ReadCapacityUnits"], 3);
    let e = w.err(
        "RestoreTableFromBackup",
        json!({"TargetTableName": "bad", "BackupArn": arn, "GlobalSecondaryIndexOverride": [
            {"IndexName": "nope", "KeySchema": [{"AttributeName": "v", "KeyType": "HASH"}], "Projection": {"ProjectionType": "ALL"}}]}),
    );
    assert_eq!(e.code, "ValidationException");
}

#[test]
fn a_table_restores_to_how_it_was_at_a_point_in_time() {
    let mut w = World::new();
    w.table("tab", false);
    let e = w.err(
        "RestoreTableToPointInTime",
        json!({"SourceTableName": "tab", "TargetTableName": "rrr", "UseLatestRestorableTime": true}),
    );
    assert_eq!(e.code, "PointInTimeRecoveryUnavailableException");
    w.ok(
        "UpdateContinuousBackups",
        json!({"TableName": "tab", "PointInTimeRecoverySpecification": {"PointInTimeRecoveryEnabled": true}}),
    );
    w.put("tab", json!({"pk": {"S": "a"}, "v": {"S": "1"}}));
    w.put("tab", json!({"pk": {"S": "b"}, "v": {"S": "1"}}));
    w.clock.advance(60_000);
    let then = START_S + 60.0;
    w.clock.advance(1_000);
    w.put("tab", json!({"pk": {"S": "a"}, "v": {"S": "2"}}));
    w.ok("DeleteItem", json!({"TableName": "tab", "Key": {"pk": {"S": "b"}}}));
    w.put("tab", json!({"pk": {"S": "c"}, "v": {"S": "1"}}));
    w.clock.advance(1_000);
    w.ok(
        "RestoreTableToPointInTime",
        json!({"SourceTableName": "tab", "TargetTableName": "then", "RestoreDateTime": then}),
    );
    assert_eq!(items(&mut w, "then"), ["a=1", "b=1"]);
    w.ok(
        "RestoreTableToPointInTime",
        json!({"SourceTableName": "tab", "TargetTableName": "now", "UseLatestRestorableTime": true}),
    );
    assert_eq!(items(&mut w, "now"), ["a=2", "c=1"]);
    let summary = w.ok("DescribeTable", json!({"TableName": "then"}))["Table"]["RestoreSummary"].clone();
    assert_eq!(summary["RestoreDateTime"].as_f64(), Some(then));
    assert!(summary.get("SourceBackupArn").is_none());
    // Before recovery was turned on there is nothing to restore to.
    let e = w.err(
        "RestoreTableToPointInTime",
        json!({"SourceTableName": "tab", "TargetTableName": "xxx", "RestoreDateTime": START_S - 10.0}),
    );
    assert_eq!(e.code, "InvalidRestoreTimeException");
    let window = w.ok("DescribeContinuousBackups", json!({"TableName": "tab"}));
    let pitr = &window["ContinuousBackupsDescription"]["PointInTimeRecoveryDescription"];
    assert_eq!(pitr["EarliestRestorableDateTime"].as_f64(), Some(START_S));
    // A day later the log has let go of the first changes, and the window says so.
    w.clock.advance(2 * 24 * 60 * 60 * 1000);
    w.put("tab", json!({"pk": {"S": "d"}, "v": {"S": "1"}}));
    let window = w.ok("DescribeContinuousBackups", json!({"TableName": "tab"}));
    let earliest = window["ContinuousBackupsDescription"]["PointInTimeRecoveryDescription"]["EarliestRestorableDateTime"]
        .as_f64()
        .unwrap();
    assert!(earliest > then + 24.0 * 3600.0, "{earliest}");
    // Turning recovery off forgets the log; on again, the window starts then.
    for on in [false, true] {
        w.ok(
            "UpdateContinuousBackups",
            json!({"TableName": "tab", "PointInTimeRecoverySpecification": {"PointInTimeRecoveryEnabled": on}}),
        );
    }
    let e = w.err(
        "RestoreTableToPointInTime",
        json!({"SourceTableName": "tab", "TargetTableName": "xxx", "RestoreDateTime": earliest}),
    );
    assert_eq!(e.code, "InvalidRestoreTimeException");
    assert!(message(&e).contains("outside of the restorable window"));
}

#[test]
fn deleting_a_table_with_recovery_on_leaves_a_system_backup() {
    let mut w = World::new();
    w.table("tab", false);
    w.ok(
        "UpdateContinuousBackups",
        json!({"TableName": "tab", "PointInTimeRecoverySpecification": {"PointInTimeRecoveryEnabled": true}}),
    );
    w.put("tab", json!({"pk": {"S": "a"}}));
    w.ok("DeleteTable", json!({"TableName": "tab"}));
    assert_eq!(w.ok("ListBackups", json!({}))["BackupSummaries"], json!([]));
    let system = w.ok("ListBackups", json!({"BackupType": "SYSTEM"}));
    let list: &Vec<Value> = system["BackupSummaries"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["BackupType"], "SYSTEM");
    assert!(list[0].get("BackupExpiryDateTime").is_some());
    // It goes after 35 days.
    w.clock.advance(36 * 24 * 60 * 60 * 1000);
    assert_eq!(w.ok("ListBackups", json!({"BackupType": "ALL"}))["BackupSummaries"], json!([]));
}

#[test]
fn backups_survive_a_restart() {
    let mut w = World::new();
    w.table("tab", false);
    w.put("tab", json!({"pk": {"S": "a"}}));
    let arn = w.ok("CreateBackup", json!({"TableName": "tab", "BackupName": "keep"}))["BackupDetails"]["BackupArn"].clone();
    w.restart();
    w.ok("RestoreTableFromBackup", json!({"TargetTableName": "rrr", "BackupArn": arn}));
    assert_eq!(items(&mut w, "rrr"), ["a=-"]);
    let e = w.err("CreateBackup", json!({"TableName": "nope", "BackupName": "xxx"}));
    assert_eq!((e.code.as_ref(), message(&e)), ("TableNotFoundException", "Table not found: nope"));
}
