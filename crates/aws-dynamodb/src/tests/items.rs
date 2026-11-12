//! Items: put, get, update, delete, conditions, return values and AWS's refusals.

use serde_json::json;

use super::{World, message};

fn world() -> World {
    let mut w = World::new();
    w.table("tab1", false);
    w
}

#[test]
fn every_type_round_trips_and_a_put_returns_the_old_item() {
    let mut w = world();
    let item = json!({
        "pk": {"S": "a"}, "n": {"N": "1.50"}, "b": {"B": "AP8="}, "flag": {"BOOL": true}, "nothing": {"NULL": true},
        "list": {"L": [{"S": "x"}, {"N": "2"}]}, "map": {"M": {"inner": {"S": "y"}}}, "ss": {"SS": ["b", "a"]}, "ns": {"NS": ["2", "1"]}
    });
    assert_eq!(
        w.ok("PutItem", json!({"TableName": "tab1", "Item": item})),
        json!({})
    );
    let got = w.ok(
        "GetItem",
        json!({"TableName": "tab1", "Key": {"pk": {"S": "a"}}, "ConsistentRead": true}),
    );
    assert_eq!(got["Item"]["n"], json!({"N": "1.5"}));
    assert_eq!(got["Item"]["ss"], json!({"SS": ["a", "b"]}));
    assert_eq!(got["Item"]["ns"], json!({"NS": ["1", "2"]}));
    assert_eq!(got["Item"]["b"], json!({"B": "AP8="}));
    assert_eq!(
        w.ok(
            "GetItem",
            json!({"TableName": "tab1", "Key": {"pk": {"S": "zz"}}})
        ),
        json!({})
    );
    let old = w.ok("PutItem", json!({"TableName": "tab1", "Item": {"pk": {"S": "a"}, "n": {"N": "2"}}, "ReturnValues": "ALL_OLD"}));
    assert_eq!(
        old["Attributes"]["list"],
        json!({"L": [{"S": "x"}, {"N": "2"}]})
    );
    let gone = w.ok(
        "DeleteItem",
        json!({"TableName": "tab1", "Key": {"pk": {"S": "a"}}, "ReturnValues": "ALL_OLD"}),
    );
    assert_eq!(
        gone["Attributes"],
        json!({"pk": {"S": "a"}, "n": {"N": "2"}})
    );
}

#[test]
fn the_recorded_update_and_its_return_values() {
    let mut w = world();
    w.put(
        "tab1",
        json!({"pk": {"S": "a"}, "n": {"N": "2"}, "gone": {"S": "x"}}),
    );
    let new = w.ok(
        "UpdateItem",
        json!({"TableName": "tab1", "Key": {"pk": {"S": "a"}}, "UpdateExpression": "SET #n = #n + :one, tags = :t ADD seen :s REMOVE gone",
            "ExpressionAttributeNames": {"#n": "n"},
            "ExpressionAttributeValues": {":one": {"N": "1"}, ":t": {"L": []}, ":s": {"SS": ["x"]}}, "ReturnValues": "ALL_NEW"}),
    );
    assert_eq!(
        new["Attributes"],
        json!({"pk": {"S": "a"}, "n": {"N": "3"}, "tags": {"L": []}, "seen": {"SS": ["x"]}})
    );
    let old = w.ok(
        "UpdateItem",
        json!({"TableName": "tab1", "Key": {"pk": {"S": "a"}}, "UpdateExpression": "SET n = :v",
            "ExpressionAttributeValues": {":v": {"N": "9"}}, "ReturnValues": "UPDATED_OLD"}),
    );
    assert_eq!(old["Attributes"], json!({"n": {"N": "3"}}));
    let newer = w.ok(
        "UpdateItem",
        json!({"TableName": "tab1", "Key": {"pk": {"S": "a"}}, "UpdateExpression": "SET n = n + :v",
            "ExpressionAttributeValues": {":v": {"N": "1"}}, "ReturnValues": "UPDATED_NEW"}),
    );
    assert_eq!(newer["Attributes"], json!({"n": {"N": "10"}}));
}

#[test]
fn an_update_creates_a_missing_item() {
    let mut w = world();
    w.ok(
        "UpdateItem",
        json!({"TableName": "tab1", "Key": {"pk": {"S": "new"}}, "UpdateExpression": "SET c = if_not_exists(c, :z)", "ExpressionAttributeValues": {":z": {"N": "0"}}}),
    );
    let got = w.ok(
        "GetItem",
        json!({"TableName": "tab1", "Key": {"pk": {"S": "new"}}}),
    );
    assert_eq!(got["Item"], json!({"pk": {"S": "new"}, "c": {"N": "0"}}));
}

#[test]
fn conditions_hold_or_fail_as_aws_says() {
    let mut w = world();
    w.put("tab1", json!({"pk": {"S": "a"}, "n": {"N": "1"}}));
    let e = w.err("PutItem", json!({"TableName": "tab1", "Item": {"pk": {"S": "a"}}, "ConditionExpression": "attribute_not_exists(pk)"}));
    assert_eq!(
        (e.code.as_ref(), message(&e)),
        (
            "ConditionalCheckFailedException",
            "The conditional request failed"
        )
    );
    assert!(e.body.is_none());
    let e = w.err(
        "PutItem",
        json!({"TableName": "tab1", "Item": {"pk": {"S": "a"}}, "ConditionExpression": "attribute_not_exists(pk)", "ReturnValuesOnConditionCheckFailure": "ALL_OLD"}),
    );
    assert_eq!(e.body.as_ref().unwrap().0[0].0, "Item");
    assert!(e.body.as_ref().unwrap().0[0].1.contains("\"n\""));
    w.ok("PutItem", json!({"TableName": "tab1", "Item": {"pk": {"S": "b"}}, "ConditionExpression": "attribute_not_exists(pk)"}));
    let e = w.err("DeleteItem", json!({"TableName": "tab1", "Key": {"pk": {"S": "a"}}, "ConditionExpression": "n > :five", "ExpressionAttributeValues": {":five": {"N": "5"}}}));
    assert_eq!(e.code, "ConditionalCheckFailedException");
    assert_eq!(
        w.ok(
            "GetItem",
            json!({"TableName": "tab1", "Key": {"pk": {"S": "a"}}})
        )["Item"]["n"],
        json!({"N": "1"})
    );
}

#[test]
fn the_recorded_refusals() {
    let mut w = world();
    let e = w.err(
        "PutItem",
        json!({"TableName": "tab1", "Item": {"other": {"S": "x"}}}),
    );
    assert_eq!(
        message(&e),
        "One or more parameter values were invalid: Missing the key pk in the item"
    );
    let e = w.err(
        "PutItem",
        json!({"TableName": "tab1", "Item": {"pk": {"N": "1"}}}),
    );
    assert_eq!(
        message(&e),
        "One or more parameter values were invalid: Type mismatch for key pk expected: S actual: N"
    );
}

#[test]
fn the_recorded_update_expression_refusals() {
    let mut w = world();
    let key = json!({"TableName": "tab1", "Key": {"pk": {"S": "a"}}});
    let update = |expr: &str, values: serde_json::Value| {
        let mut r = key.clone();
        r["UpdateExpression"] = json!(expr);
        if !values.is_null() {
            r["ExpressionAttributeValues"] = values;
        }
        r
    };
    let e = w.err("UpdateItem", update("SET", json!(null)));
    assert_eq!(
        message(&e),
        "1 validation error detected: Invalid UpdateExpression: Syntax error; token: \"<EOF>\", near: \"SET\""
    );
    let e = w.err("UpdateItem", update("SET n = :missing", json!(null)));
    assert_eq!(
        message(&e),
        "1 validation error detected: Invalid UpdateExpression: An expression attribute value used in expression is not defined; attribute value: :missing"
    );
    let e = w.err(
        "UpdateItem",
        update(
            "SET n = :v",
            json!({":v": {"N": "1"}, ":unused": {"N": "2"}}),
        ),
    );
    assert_eq!(
        message(&e),
        "1 validation error detected: Value provided in ExpressionAttributeValues unused in expressions: keys: {:unused}"
    );
    let e = w.err(
        "UpdateItem",
        update("SET name = :v", json!({":v": {"S": "x"}})),
    );
    assert_eq!(
        message(&e),
        "1 validation error detected: Invalid UpdateExpression: Attribute name is a reserved keyword; reserved keyword: name"
    );
    let e = w.err(
        "UpdateItem",
        update("SET pk = :v", json!({":v": {"S": "b"}})),
    );
    assert_eq!(
        message(&e),
        "One or more parameter values were invalid: Cannot update attribute pk. This attribute is part of the key"
    );
    let e = w.err(
        "PutItem",
        json!({"TableName": "tab1", "Item": {"pk": {"S": "a"}, "s": {"SS": []}}}),
    );
    assert_eq!(
        message(&e),
        "1 validation error detected: One or more parameter values were invalid: An string set  may not be empty"
    );
}

#[test]
fn key_and_item_limits() {
    let mut w = world();
    let e = w.err(
        "PutItem",
        json!({"TableName": "tab1", "Item": {"pk": {"S": ""}}}),
    );
    assert!(message(&e).contains("cannot contain an empty string value. Key: pk"));
    let e = w.err(
        "PutItem",
        json!({"TableName": "tab1", "Item": {"pk": {"S": "x".repeat(2049)}}}),
    );
    assert!(
        message(&e).contains("Size of hashkey has exceeded the maximum size limit of2048 bytes")
    );
    let big = "x".repeat(401 * 1024);
    let e = w.err(
        "PutItem",
        json!({"TableName": "tab1", "Item": {"pk": {"S": "a"}, "blob": {"S": big}}}),
    );
    assert_eq!(
        message(&e),
        "Item size has exceeded the maximum allowed size"
    );
    let e = w.err(
        "GetItem",
        json!({"TableName": "tab1", "Key": {"pk": {"S": "a"}, "extra": {"S": "b"}}}),
    );
    assert_eq!(
        message(&e),
        "The provided key element does not match the schema"
    );
    let e = w.err(
        "GetItem",
        json!({"TableName": "tab1", "Key": {"pk": {"N": "1"}}}),
    );
    assert_eq!(
        message(&e),
        "The provided key element does not match the schema"
    );
    let mut deep = json!({"S": "x"});
    for _ in 0..40 {
        deep = json!({"L": [deep]});
    }
    let e = w.err(
        "PutItem",
        json!({"TableName": "tab1", "Item": {"pk": {"S": "a"}, "deep": deep}}),
    );
    assert!(message(&e).contains("Nesting Levels"));
}

#[test]
fn projection_on_a_get() {
    let mut w = world();
    w.put("tab1", json!({"pk": {"S": "a"}, "x": {"N": "1"}, "m": {"M": {"k": {"S": "v"}, "z": {"S": "w"}}}, "l": {"L": [{"S": "p"}, {"S": "q"}]}}));
    let got = w.ok("GetItem", json!({"TableName": "tab1", "Key": {"pk": {"S": "a"}}, "ProjectionExpression": "x, m.k, l[1]"}));
    assert_eq!(
        got["Item"],
        json!({"x": {"N": "1"}, "m": {"M": {"k": {"S": "v"}}}, "l": {"L": [{"S": "q"}]}})
    );
}

#[test]
fn legacy_parameters_are_refused_by_name_and_never_ignored() {
    let mut w = world();
    for (op, extra) in [
        (
            "PutItem",
            json!({"Item": {"pk": {"S": "a"}}, "Expected": {"pk": {"Exists": false}}}),
        ),
        (
            "PutItem",
            json!({"Item": {"pk": {"S": "a"}}, "ConditionalOperator": "OR"}),
        ),
        (
            "UpdateItem",
            json!({"Key": {"pk": {"S": "a"}}, "AttributeUpdates": {"n": {"Action": "PUT", "Value": {"N": "1"}}}}),
        ),
        (
            "GetItem",
            json!({"Key": {"pk": {"S": "a"}}, "AttributesToGet": ["pk"]}),
        ),
        (
            "Scan",
            json!({"ScanFilter": {"n": {"ComparisonOperator": "NULL"}}}),
        ),
        (
            "Query",
            json!({"KeyConditions": {"pk": {"ComparisonOperator": "EQ", "AttributeValueList": [{"S": "a"}]}}}),
        ),
    ] {
        let mut req = json!({"TableName": "tab1"});
        for (k, v) in extra.as_object().unwrap() {
            req[k] = v.clone();
        }
        let e = w.err(op, req);
        assert_eq!((e.status, e.code.as_ref()), (501, "NotImplemented"), "{op}");
        assert!(message(&e).contains("legacy parameter"));
    }
    assert_eq!(
        w.ok(
            "GetItem",
            json!({"TableName": "tab1", "Key": {"pk": {"S": "a"}}})
        ),
        json!({})
    );
}

#[test]
fn consumed_capacity_is_reported_when_asked() {
    let mut w = world();
    let out = w.ok(
        "PutItem",
        json!({"TableName": "tab1", "Item": {"pk": {"S": "a"}}, "ReturnConsumedCapacity": "TOTAL"}),
    );
    assert_eq!(out["ConsumedCapacity"]["CapacityUnits"].as_f64(), Some(1.0));
}
