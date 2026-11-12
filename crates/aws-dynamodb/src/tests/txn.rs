//! Batches and transactions, time to live and streams.

use serde_json::{Value, json};

use super::{World, message};

fn world() -> World {
    let mut w = World::new();
    w.table("tab3", false);
    w
}

fn put(k: &str) -> Value {
    json!({"PutRequest": {"Item": {"pk": {"S": k}}}})
}

#[test]
fn batch_write_puts_and_deletes_and_refuses_duplicates() {
    let mut w = world();
    let out = w.ok(
        "BatchWriteItem",
        json!({"RequestItems": {"tab3": [put("a"), put("b"), put("c")]}}),
    );
    assert_eq!(out["UnprocessedItems"], json!({}));
    w.ok("BatchWriteItem", json!({"RequestItems": {"tab3": [{"DeleteRequest": {"Key": {"pk": {"S": "a"}}}}, put("d")]}}));
    assert_eq!(
        w.ok(
            "Scan",
            json!({"TableName": "tab3", "Select": "COUNT", "ConsistentRead": true})
        )["Count"],
        3
    );
    let e = w.err(
        "BatchWriteItem",
        json!({"RequestItems": {"tab3": [put("z"), put("z")]}}),
    );
    assert_eq!(
        message(&e),
        "Provided list of item keys contains duplicates"
    );
    let e = w.err("BatchWriteItem", json!({"RequestItems": {"tab3": (0..26).map(|i| put(&format!("k{i}"))).collect::<Vec<_>>()}}));
    assert!(
        message(&e).contains("Member must have length less than or equal to 25")
            || message(&e).contains("25")
    );
}

#[test]
fn a_batch_with_one_bad_request_writes_nothing() {
    let mut w = world();
    let bad = json!({"PutRequest": {"Item": {"pk": {"N": "1"}}}});
    let e = w.err(
        "BatchWriteItem",
        json!({"RequestItems": {"tab3": [put("one"), bad]}}),
    );
    assert!(message(&e).contains("Type mismatch for key pk"));
    assert_eq!(
        w.ok("Scan", json!({"TableName": "tab3", "Select": "COUNT"}))["Count"],
        0
    );
    let e = w.err(
        "BatchWriteItem",
        json!({"RequestItems": {"tab3": [put("one")], "absent": [put("x")]}}),
    );
    assert_eq!(e.code, "ResourceNotFoundException");
    assert_eq!(
        w.ok("Scan", json!({"TableName": "tab3", "Select": "COUNT"}))["Count"],
        0
    );
}

#[test]
fn batch_get_returns_what_exists() {
    let mut w = world();
    w.put("tab3", json!({"pk": {"S": "a"}, "n": {"N": "2"}}));
    w.put("tab3", json!({"pk": {"S": "b"}}));
    let out = w.ok(
        "BatchGetItem",
        json!({"RequestItems": {"tab3": {"Keys": [{"pk": {"S": "a"}}, {"pk": {"S": "b"}}, {"pk": {"S": "zz"}}], "ConsistentRead": true}}}),
    );
    assert_eq!(
        out["Responses"]["tab3"],
        json!([{"pk": {"S": "a"}, "n": {"N": "2"}}, {"pk": {"S": "b"}}])
    );
    assert_eq!(out["UnprocessedKeys"], json!({}));
    let e = w.err(
        "BatchGetItem",
        json!({"RequestItems": {"tab3": {"Keys": [{"pk": {"S": "a"}}, {"pk": {"S": "a"}}]}}}),
    );
    assert_eq!(
        message(&e),
        "Provided list of item keys contains duplicates"
    );
    let keys: Vec<Value> = (0..101)
        .map(|i| json!({"pk": {"S": format!("k{i}")}}))
        .collect();
    let e = w.err(
        "BatchGetItem",
        json!({"RequestItems": {"tab3": {"Keys": keys}}}),
    );
    assert!(message(&e).contains("100") || message(&e).contains("Too many"));
}

#[test]
fn a_transaction_applies_whole_or_not_at_all() {
    let mut w = world();
    w.put("tab3", json!({"pk": {"S": "a"}, "n": {"N": "1"}}));
    w.ok(
        "TransactWriteItems",
        json!({"TransactItems": [
            {"Put": {"TableName": "tab3", "Item": {"pk": {"S": "b"}}}},
            {"Update": {"TableName": "tab3", "Key": {"pk": {"S": "a"}}, "UpdateExpression": "SET n = :v", "ExpressionAttributeValues": {":v": {"N": "2"}}}}]}),
    );
    assert_eq!(
        w.ok(
            "GetItem",
            json!({"TableName": "tab3", "Key": {"pk": {"S": "a"}}})
        )["Item"]["n"],
        json!({"N": "2"})
    );
    let e = w.err(
        "TransactWriteItems",
        json!({"TransactItems": [
            {"Put": {"TableName": "tab3", "Item": {"pk": {"S": "c"}}}},
            {"ConditionCheck": {"TableName": "tab3", "Key": {"pk": {"S": "a"}}, "ConditionExpression": "n = :v", "ExpressionAttributeValues": {":v": {"N": "999"}}}}]}),
    );
    assert_eq!(e.code, "TransactionCanceledException");
    assert_eq!(
        message(&e),
        "Transaction cancelled, please refer cancellation reasons for specific reasons [None, ConditionalCheckFailed]"
    );
    let reasons: Value = serde_json::from_str(&e.body.as_ref().unwrap().0[0].1).unwrap();
    assert_eq!(
        reasons,
        json!([{"Code": "None"}, {"Code": "ConditionalCheckFailed", "Message": "The conditional request failed"}])
    );
    assert_eq!(
        w.ok(
            "GetItem",
            json!({"TableName": "tab3", "Key": {"pk": {"S": "c"}}})
        ),
        json!({})
    );
}

#[test]
fn a_cancelled_transaction_can_return_the_item_that_failed() {
    let mut w = world();
    w.put("tab3", json!({"pk": {"S": "a"}, "n": {"N": "1"}}));
    let e = w.err(
        "TransactWriteItems",
        json!({"TransactItems": [{"Delete": {"TableName": "tab3", "Key": {"pk": {"S": "a"}}, "ConditionExpression": "n = :v",
            "ExpressionAttributeValues": {":v": {"N": "5"}}, "ReturnValuesOnConditionCheckFailure": "ALL_OLD"}}]}),
    );
    let reasons: Value = serde_json::from_str(&e.body.as_ref().unwrap().0[0].1).unwrap();
    assert_eq!(
        reasons[0]["Item"],
        json!({"pk": {"S": "a"}, "n": {"N": "1"}})
    );
}

#[test]
fn transaction_rules() {
    let mut w = world();
    let same = json!({"TransactItems": [
        {"Put": {"TableName": "tab3", "Item": {"pk": {"S": "a"}}}},
        {"Delete": {"TableName": "tab3", "Key": {"pk": {"S": "a"}}}}]});
    assert_eq!(
        message(&w.err("TransactWriteItems", same)),
        "Transaction request cannot include multiple operations on one item"
    );
    let many: Vec<Value> = (0..101)
        .map(|i| json!({"Put": {"TableName": "tab3", "Item": {"pk": {"S": format!("k{i}")}}}}))
        .collect();
    assert!(
        w.err("TransactWriteItems", json!({"TransactItems": many}))
            .code
            == "ValidationException"
    );
    let token = |v: &str| {
        json!({"ClientRequestToken": "tok-1", "TransactItems": [{"Update": {"TableName": "tab3", "Key": {"pk": {"S": "ctr"}},
            "UpdateExpression": "ADD n :one", "ExpressionAttributeValues": {":one": {"N": v}}}}]})
    };
    w.ok("TransactWriteItems", token("1"));
    w.ok("TransactWriteItems", token("1"));
    assert_eq!(
        w.ok(
            "GetItem",
            json!({"TableName": "tab3", "Key": {"pk": {"S": "ctr"}}})
        )["Item"]["n"],
        json!({"N": "1"})
    );
    assert_eq!(
        w.err("TransactWriteItems", token("2")).code,
        "IdempotentParameterMismatchException"
    );
    w.clock.advance(11 * 60 * 1000);
    w.ok("TransactWriteItems", token("2"));
    assert_eq!(
        w.ok(
            "GetItem",
            json!({"TableName": "tab3", "Key": {"pk": {"S": "ctr"}}})
        )["Item"]["n"],
        json!({"N": "3"})
    );
}

#[test]
fn a_batch_get_response_stops_at_sixteen_megabytes_and_hands_back_the_rest() {
    let mut w = world();
    let blob = "x".repeat(390 * 1024);
    for i in 0..45 {
        w.put(
            "tab3",
            json!({"pk": {"S": format!("k{i}")}, "blob": {"S": blob}}),
        );
    }
    let keys: Vec<Value> = (0..45)
        .map(|i| json!({"pk": {"S": format!("k{i}")}}))
        .collect();
    let out = w.ok(
        "BatchGetItem",
        json!({"RequestItems": {"tab3": {"Keys": keys}}}),
    );
    let got = out["Responses"]["tab3"].as_array().unwrap().len();
    let left = out["UnprocessedKeys"]["tab3"]["Keys"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(got + left, 45);
    assert!(got >= 41 && left >= 1, "{got} returned, {left} handed back");
}

#[test]
fn a_transaction_carries_at_most_four_megabytes() {
    let mut w = world();
    let blob = "x".repeat(390 * 1024);
    let puts = |n: usize| -> Vec<Value> {
        (0..n)
            .map(|i| json!({"Put": {"TableName": "tab3", "Item": {"pk": {"S": format!("k{i}")}, "blob": {"S": blob}}}}))
            .collect()
    };
    w.ok("TransactWriteItems", json!({"TransactItems": puts(10)}));
    let e = w.err("TransactWriteItems", json!({"TransactItems": puts(11)}));
    assert!(message(&e).contains("4 MB"));
    assert_eq!(
        w.ok(
            "GetItem",
            json!({"TableName": "tab3", "Key": {"pk": {"S": "k10"}}})
        ),
        json!({})
    );
}

#[test]
fn transact_get_keeps_the_order() {
    let mut w = world();
    w.put("tab3", json!({"pk": {"S": "a"}, "n": {"N": "1"}}));
    let out = w.ok(
        "TransactGetItems",
        json!({"TransactItems": [{"Get": {"TableName": "tab3", "Key": {"pk": {"S": "zz"}}}}, {"Get": {"TableName": "tab3", "Key": {"pk": {"S": "a"}}, "ProjectionExpression": "n"}}]}),
    );
    assert_eq!(out["Responses"], json!([{}, {"Item": {"n": {"N": "1"}}}]));
}

#[test]
fn expired_items_are_deleted_and_a_stream_says_the_service_did_it() {
    let mut w = world();
    w.ok("UpdateTimeToLive", json!({"TableName": "tab3", "TimeToLiveSpecification": {"Enabled": true, "AttributeName": "expires"}}));
    w.ok("UpdateTable", json!({"TableName": "tab3", "StreamSpecification": {"StreamEnabled": true, "StreamViewType": "NEW_AND_OLD_IMAGES"}}));
    let now = doze_runtime::Clock::now_ms(&*w.clock) / 1000;
    w.put(
        "tab3",
        json!({"pk": {"S": "soon"}, "expires": {"N": (now + 60).to_string()}}),
    );
    w.put("tab3", json!({"pk": {"S": "never"}}));
    w.put(
        "tab3",
        json!({"pk": {"S": "past"}, "expires": {"N": (now - 60).to_string()}}),
    );
    assert_eq!(
        w.ok("Scan", json!({"TableName": "tab3", "Select": "COUNT"}))["Count"],
        3,
        "an expired item lingers until the sweep, as on AWS"
    );
    w.clock.advance(1_000);
    assert_eq!(
        w.ok("Scan", json!({"TableName": "tab3", "Select": "COUNT"}))["Count"],
        2,
        "the sweep removed it"
    );
    w.clock.advance(61_000);
    assert_eq!(
        w.ok("Scan", json!({"TableName": "tab3", "Select": "COUNT"}))["Count"],
        1
    );
    let arn = w.ok("DescribeTable", json!({"TableName": "tab3"}))["Table"]["LatestStreamArn"]
        .as_str()
        .unwrap()
        .to_owned();
    let batch = w.core.as_ref().unwrap().read_stream(&arn, 0, 100).unwrap();
    let removed: Vec<_> = batch
        .records
        .iter()
        .filter(|r| r.by_ttl)
        .map(|r| r.to_event("us-east-1"))
        .collect();
    assert_eq!(removed.len(), 2);
    assert_eq!(removed[0]["eventName"], "REMOVE");
    assert_eq!(
        removed[0]["userIdentity"],
        json!({"type": "Service", "principalId": "dynamodb.amazonaws.com"})
    );
}

#[test]
fn a_stream_records_changes_with_the_images_its_view_asks_for() {
    let mut w = world();
    let on = w.ok("UpdateTable", json!({"TableName": "tab3", "StreamSpecification": {"StreamEnabled": true, "StreamViewType": "NEW_AND_OLD_IMAGES"}}));
    let arn = on["TableDescription"]["LatestStreamArn"]
        .as_str()
        .unwrap()
        .to_owned();
    w.put("tab3", json!({"pk": {"S": "a"}, "n": {"N": "1"}}));
    w.put("tab3", json!({"pk": {"S": "a"}, "n": {"N": "1"}}));
    w.put("tab3", json!({"pk": {"S": "a"}, "n": {"N": "2"}}));
    w.ok(
        "DeleteItem",
        json!({"TableName": "tab3", "Key": {"pk": {"S": "a"}}}),
    );
    w.ok(
        "DeleteItem",
        json!({"TableName": "tab3", "Key": {"pk": {"S": "a"}}}),
    );
    let batch = w.core.as_ref().unwrap().read_stream(&arn, 0, 100).unwrap();
    let events: Vec<_> = batch
        .records
        .iter()
        .map(|r| r.to_event("us-east-1"))
        .collect();
    let names: Vec<_> = events
        .iter()
        .map(|e| e["eventName"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["INSERT", "MODIFY", "REMOVE"],
        "an identical put and a delete of nothing write no record"
    );
    assert_eq!(events[1]["dynamodb"]["OldImage"]["n"], json!({"N": "1"}));
    assert_eq!(events[1]["dynamodb"]["NewImage"]["n"], json!({"N": "2"}));
    assert_eq!(events[0]["dynamodb"]["Keys"], json!({"pk": {"S": "a"}}));
    assert!(
        events[0]["dynamodb"]["SequenceNumber"]
            .as_str()
            .unwrap()
            .len()
            >= 21
    );
    let after = w
        .core
        .as_ref()
        .unwrap()
        .read_stream(&arn, batch.records[0].seq, 1)
        .unwrap();
    assert_eq!(after.records.len(), 1);
    assert_eq!(after.records[0].seq, batch.records[1].seq);
    assert!(
        w.core
            .as_ref()
            .unwrap()
            .read_stream("arn:aws:dynamodb:us-east-1:0:table/tab3/stream/other", 0, 1)
            .is_err()
    );
}
