//! State survives a stop and a start; a call writes only what it changes.

use serde_json::json;

use super::World;

#[test]
fn tables_items_indexes_streams_and_settings_survive_a_restart() {
    let mut w = World::new();
    w.ok(
        "CreateTable",
        json!({"TableName": "docs", "BillingMode": "PAY_PER_REQUEST", "Tags": [{"Key": "team", "Value": "a"}],
            "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
            "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}, {"AttributeName": "owner", "AttributeType": "S"}],
            "GlobalSecondaryIndexes": [{"IndexName": "by-owner", "KeySchema": [{"AttributeName": "owner", "KeyType": "HASH"}], "Projection": {"ProjectionType": "ALL"}}],
            "StreamSpecification": {"StreamEnabled": true, "StreamViewType": "NEW_IMAGE"}}),
    );
    w.ok("UpdateContinuousBackups", json!({"TableName": "docs", "PointInTimeRecoverySpecification": {"PointInTimeRecoveryEnabled": true, "RecoveryPeriodInDays": 10}}));
    w.put("docs", json!({"pk": {"S": "a"}, "owner": {"S": "ada"}}));
    let before = w.ok("DescribeTable", json!({"TableName": "docs"}));
    w.restart();
    let after = w.ok("DescribeTable", json!({"TableName": "docs"}));
    assert_eq!(before["Table"]["TableId"], after["Table"]["TableId"]);
    assert_eq!(
        before["Table"]["LatestStreamArn"],
        after["Table"]["LatestStreamArn"]
    );
    assert_eq!(
        w.ok(
            "GetItem",
            json!({"TableName": "docs", "Key": {"pk": {"S": "a"}}})
        )["Item"]["owner"],
        json!({"S": "ada"})
    );
    let by = w.ok("Query", json!({"TableName": "docs", "IndexName": "by-owner", "KeyConditionExpression": "#o = :o", "ExpressionAttributeNames": {"#o": "owner"}, "ExpressionAttributeValues": {":o": {"S": "ada"}}}));
    assert_eq!(by["Count"], 1);
    let arn = after["Table"]["LatestStreamArn"].as_str().unwrap();
    assert_eq!(
        w.core
            .as_ref()
            .unwrap()
            .read_stream(arn, 0, 10)
            .unwrap()
            .records
            .len(),
        1
    );
    let tags = w.ok(
        "ListTagsOfResource",
        json!({"ResourceArn": "arn:aws:dynamodb:us-east-1:000000000000:table/docs"}),
    );
    assert_eq!(tags["Tags"], json!([{"Key": "team", "Value": "a"}]));
    let pitr = w.ok("DescribeContinuousBackups", json!({"TableName": "docs"}));
    assert_eq!(
        pitr["ContinuousBackupsDescription"]["PointInTimeRecoveryDescription"]["RecoveryPeriodInDays"],
        10
    );
    // The sequence goes on from where it was.
    w.put("docs", json!({"pk": {"S": "b"}}));
    let all = w
        .core
        .as_ref()
        .unwrap()
        .read_stream(arn, 0, 10)
        .unwrap()
        .records;
    assert_eq!(all.iter().map(|r| r.seq).collect::<Vec<_>>(), [1, 2]);
}

#[test]
fn every_call_leaves_what_the_next_one_reads() {
    let mut w = World::new();
    w.table("tab", true);
    let calls = [
        (
            "PutItem",
            json!({"TableName": "tab", "Item": {"pk": {"S": "a"}, "sk": {"N": "1"}}}),
        ),
        (
            "UpdateItem",
            json!({"TableName": "tab", "Key": {"pk": {"S": "a"}, "sk": {"N": "1"}}, "UpdateExpression": "SET x = :v", "ExpressionAttributeValues": {":v": {"S": "y"}}}),
        ),
        (
            "DeleteItem",
            json!({"TableName": "tab", "Key": {"pk": {"S": "a"}, "sk": {"N": "1"}}}),
        ),
        (
            "PutItem",
            json!({"TableName": "tab", "Item": {"pk": {"S": "b"}, "sk": {"N": "2"}}}),
        ),
    ];
    for (op, req) in calls {
        w.ok(op, req);
        let before = w.ok("Scan", json!({"TableName": "tab"}));
        w.restart();
        assert_eq!(
            w.ok("Scan", json!({"TableName": "tab"})),
            before,
            "after {op}"
        );
    }
}

#[test]
fn a_deleted_table_takes_its_items_with_it() {
    let mut w = World::new();
    w.table("tab", false);
    w.put("tab", json!({"pk": {"S": "a"}}));
    w.ok("DeleteTable", json!({"TableName": "tab"}));
    w.restart();
    w.table("tab", false);
    assert_eq!(w.ok("Scan", json!({"TableName": "tab"}))["Count"], 0);
}

/// A call does not wait for the one before it to be on disk: the next call reads the queued write, and the write's own answer waits
/// for its commit; after the commit and a restart the item is there.
#[test]
fn a_read_sees_a_put_still_queued_and_the_put_answers_once_it_is_on_disk() {
    let mut w = World::new();
    w.table("tab", false);
    let (release, held) = w.core.as_ref().unwrap().disk.hold();
    let item = json!({"TableName": "tab", "Item": {"pk": {"S": "a"}, "v": {"S": "one"}}});
    let (put, acks) = w.call_queued("PutItem", &item);
    put.unwrap();
    assert!(
        !acks.is_empty(),
        "the put's answer has a commit to wait for"
    );
    assert!(
        w.core.as_ref().unwrap().disk.queued(),
        "and it is still queued"
    );
    let key = json!({"TableName": "tab", "Key": {"pk": {"S": "a"}}});
    let (got, none) = w.call_queued("GetItem", &key);
    assert_eq!(got.unwrap()["Item"]["v"], json!({"S": "one"}));
    assert!(none.is_empty(), "a read writes nothing");
    let (scanned, _) = w.call_queued("Scan", &json!({"TableName": "tab"}));
    assert_eq!(scanned.unwrap()["Count"], 1);
    let (deleted, more) = w.call_queued("DeleteItem", &key);
    deleted.unwrap();
    let (gone, _) = w.call_queued("GetItem", &key);
    assert!(
        gone.unwrap().get("Item").is_none(),
        "the queued delete is read too"
    );
    let (back, last) = w.call_queued("PutItem", &item);
    back.unwrap();
    release.send(()).unwrap();
    held.wait().unwrap();
    for ack in acks.into_iter().chain(more).chain(last) {
        w.rt.block_on(ack).unwrap();
    }
    assert!(!w.core.as_ref().unwrap().disk.queued());
    w.restart();
    assert_eq!(w.ok("GetItem", key)["Item"]["v"], json!({"S": "one"}));
}
