//! The legacy parameters: translated into expressions and run as they would be, and refused where the developer guide says.

use serde_json::{Value, json};

use super::{World, message};

fn world() -> World {
    let mut w = World::new();
    w.table("tab", true);
    for (sk, n, tag) in [(1, 10, "red"), (2, 20, "blue"), (3, 30, "red")] {
        w.put(
            "tab",
            json!({"pk": {"S": "a"}, "sk": {"N": sk.to_string()}, "n": {"N": n.to_string()}, "tag": {"S": tag}}),
        );
    }
    w
}

fn sks(out: &Value) -> Vec<String> {
    out["Items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["sk"]["N"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn query_with_key_conditions_filter_and_attributes_to_get() {
    let mut w = world();
    let out = w.ok(
        "Query",
        json!({"TableName": "tab",
            "KeyConditions": {
                "pk": {"ComparisonOperator": "EQ", "AttributeValueList": [{"S": "a"}]},
                "sk": {"ComparisonOperator": "BETWEEN", "AttributeValueList": [{"N": "1"}, {"N": "3"}]}},
            "QueryFilter": {"tag": {"ComparisonOperator": "EQ", "AttributeValueList": [{"S": "red"}]}},
            "AttributesToGet": ["sk", "n"]}),
    );
    assert_eq!(sks(&out), ["1", "3"]);
    assert_eq!(out["Items"][0], json!({"sk": {"N": "1"}, "n": {"N": "10"}}));
    assert_eq!(out["ScannedCount"], 3);
    let or = w.ok(
        "Query",
        json!({"TableName": "tab",
            "KeyConditions": {"pk": {"ComparisonOperator": "EQ", "AttributeValueList": [{"S": "a"}]}},
            "QueryFilter": {
                "n": {"ComparisonOperator": "GT", "AttributeValueList": [{"N": "25"}]},
                "tag": {"ComparisonOperator": "EQ", "AttributeValueList": [{"S": "blue"}]}},
            "ConditionalOperator": "OR"}),
    );
    assert_eq!(sks(&or), ["2", "3"]);
}

#[test]
fn scan_filter_operators() {
    let mut w = world();
    let scan = |w: &mut World, filter: Value| {
        sks(&w.ok("Scan", json!({"TableName": "tab", "ScanFilter": filter})))
    };
    assert_eq!(
        scan(
            &mut w,
            json!({"tag": {"ComparisonOperator": "BEGINS_WITH", "AttributeValueList": [{"S": "bl"}]}})
        ),
        ["2"]
    );
    assert_eq!(
        scan(
            &mut w,
            json!({"n": {"ComparisonOperator": "IN", "AttributeValueList": [{"N": "10"}, {"N": "30"}]}})
        ),
        ["1", "3"]
    );
    assert_eq!(
        scan(&mut w, json!({"gone": {"ComparisonOperator": "NULL"}})).len(),
        3
    );
    assert!(scan(&mut w, json!({"gone": {"ComparisonOperator": "NOT_NULL"}})).is_empty());
    assert_eq!(
        scan(
            &mut w,
            json!({"tag": {"ComparisonOperator": "CONTAINS", "AttributeValueList": [{"S": "e"}]}})
        ),
        ["1", "2", "3"]
    );
}

#[test]
fn expected_guards_writes() {
    let mut w = world();
    let key = json!({"pk": {"S": "a"}, "sk": {"N": "1"}});
    let e = w.err(
        "PutItem",
        json!({"TableName": "tab", "Item": {"pk": {"S": "a"}, "sk": {"N": "1"}}, "Expected": {"sk": {"Exists": false}}}),
    );
    assert_eq!(e.code, "ConditionalCheckFailedException");
    w.ok(
        "DeleteItem",
        json!({"TableName": "tab", "Key": key, "Expected": {"n": {"Value": {"N": "10"}}, "tag": {"Exists": true, "Value": {"S": "red"}}}}),
    );
    let e = w.err(
        "PutItem",
        json!({"TableName": "tab", "Item": {"pk": {"S": "a"}, "sk": {"N": "2"}},
            "Expected": {"n": {"ComparisonOperator": "LT", "AttributeValueList": [{"N": "5"}]}, "tag": {"Exists": false}},
            "ConditionalOperator": "OR"}),
    );
    assert_eq!(e.code, "ConditionalCheckFailedException");
}

#[test]
fn attribute_updates_put_add_and_delete() {
    let mut w = world();
    let key = json!({"pk": {"S": "a"}, "sk": {"N": "2"}});
    let out = w.ok(
        "UpdateItem",
        json!({"TableName": "tab", "Key": key, "ReturnValues": "ALL_NEW",
            "AttributeUpdates": {
                "n": {"Action": "ADD", "Value": {"N": "5"}},
                "tags": {"Action": "ADD", "Value": {"SS": ["x", "y"]}},
                "tag": {"Action": "DELETE"},
                "note": {"Value": {"S": "hi"}}}}),
    );
    assert_eq!(
        out["Attributes"],
        json!({"pk": {"S": "a"}, "sk": {"N": "2"}, "n": {"N": "25"}, "tags": {"SS": ["x", "y"]}, "note": {"S": "hi"}})
    );
    let out = w.ok(
        "UpdateItem",
        json!({"TableName": "tab", "Key": key, "ReturnValues": "UPDATED_NEW",
            "AttributeUpdates": {"tags": {"Action": "DELETE", "Value": {"SS": ["x"]}}},
            "Expected": {"note": {"Value": {"S": "hi"}}}}),
    );
    assert_eq!(out["Attributes"], json!({"tags": {"SS": ["y"]}}));
    // DELETE on an item that is not there: nothing happens, and no item is made.
    let absent = json!({"pk": {"S": "b"}, "sk": {"N": "9"}});
    w.ok(
        "UpdateItem",
        json!({"TableName": "tab", "Key": absent, "AttributeUpdates": {"x": {"Action": "DELETE"}}}),
    );
    assert_eq!(
        w.ok("GetItem", json!({"TableName": "tab", "Key": absent})),
        json!({})
    );
    w.ok(
        "UpdateItem",
        json!({"TableName": "tab", "Key": absent, "AttributeUpdates": {"c": {"Action": "ADD", "Value": {"N": "3"}}}}),
    );
    assert_eq!(
        w.ok(
            "GetItem",
            json!({"TableName": "tab", "Key": absent, "AttributesToGet": ["c"]})
        ),
        json!({"Item": {"c": {"N": "3"}}})
    );
}

#[test]
fn batch_get_takes_attributes_to_get_per_table() {
    let mut w = world();
    let out = w.ok(
        "BatchGetItem",
        json!({"RequestItems": {"tab": {"Keys": [{"pk": {"S": "a"}, "sk": {"N": "1"}}], "AttributesToGet": ["tag"]}}}),
    );
    assert_eq!(out["Responses"]["tab"], json!([{"tag": {"S": "red"}}]));
}

#[test]
fn mixing_and_malformed_legacy_parameters_are_refused() {
    let mut w = world();
    let key = json!({"pk": {"S": "a"}, "sk": {"N": "1"}});
    let e = w.err(
        "GetItem",
        json!({"TableName": "tab", "Key": key, "AttributesToGet": ["n"], "ProjectionExpression": "n"}),
    );
    assert_eq!(e.code, "ValidationException");
    assert_eq!(
        message(&e),
        "Can not use both expression and non-expression parameters in the same request: Non-expression parameters: {AttributesToGet} Expression parameters: {ProjectionExpression}"
    );
    // KeyConditions beside a FilterExpression is accepted (recorded 2026-11-19); the two sets of placeholders join.
    let r = w.ok(
        "Query",
        json!({"TableName": "tab", "KeyConditions": {"pk": {"ComparisonOperator": "EQ", "AttributeValueList": [{"S": "a"}]}},
            "FilterExpression": "n > :v", "ExpressionAttributeValues": {":v": {"N": "15"}}}),
    );
    assert_eq!(r["Count"], 2, "{r}");
    assert_eq!(r["ScannedCount"], 3, "{r}");
    let e = w.err(
        "Query",
        json!({"TableName": "tab", "KeyConditions": {"pk": {"ComparisonOperator": "EQ", "AttributeValueList": [{"S": "a"}]}},
            "KeyConditionExpression": "pk = :v", "ExpressionAttributeValues": {":v": {"S": "a"}}}),
    );
    assert!(message(&e).contains("Non-expression parameters: {KeyConditions}"));
    // Recorded 2026-11-19: a write operation says it behind `1 validation error detected: `.
    let e = w.err(
        "PutItem",
        json!({"TableName": "tab", "Item": key, "Expected": {"n": {"Exists": false}},
            "ConditionExpression": "attribute_not_exists(pk)"}),
    );
    assert_eq!(
        message(&e),
        "1 validation error detected: Can not use both expression and non-expression parameters in the same request: Non-expression parameters: {Expected} Expression parameters: {ConditionExpression}"
    );
}

#[test]
fn malformed_legacy_parameters_are_refused() {
    let mut w = world();
    let cases = [
        (
            json!({"KeyConditions": {"pk": {"ComparisonOperator": "EQ", "AttributeValueList": []}}}),
            "Invalid number of argument(s) for the EQ ComparisonOperator",
        ),
        (
            json!({"KeyConditions": {"pk": {"ComparisonOperator": "NE", "AttributeValueList": [{"S": "a"}]}}}),
            "One or more parameter values were invalid: Attempted conditional constraint is not an indexable operation",
        ),
        (
            json!({"KeyConditions": {"pk": {"ComparisonOperator": "EQ", "AttributeValueList": [{"S": "a"}]}},
                "QueryFilter": {"n": {"ComparisonOperator": "BEGINS_WITH", "AttributeValueList": [{"N": "1"}]}}}),
            "ComparisonOperator BEGINS_WITH is not valid for N AttributeValue type",
        ),
        (
            json!({"KeyConditions": {"pk": {"ComparisonOperator": "EQ", "AttributeValueList": [{"S": "a"}]}},
                "QueryFilter": {"n": {"ComparisonOperator": "NULL"}}, "ConditionalOperator": "AND"}),
            "ConditionalOperator can only be used when Filter or Expected has two or more elements",
        ),
    ];
    for (extra, want) in cases {
        let mut req = json!({"TableName": "tab"});
        for (k, v) in extra.as_object().unwrap() {
            req[k] = v.clone();
        }
        let e = w.err("Query", req);
        assert!(message(&e).contains(want), "{} / {want}", message(&e));
    }
    // Recorded 2026-11-19: this one carries no `One or more parameter values were invalid`.
    let e = w.err(
        "Query",
        json!({"TableName": "tab", "KeyConditions": {"pk": {"ComparisonOperator": "EQ", "AttributeValueList": [{"S": "a"}]}},
            "QueryFilter": {"n": {"ComparisonOperator": "NULL"}}, "ConditionalOperator": "AND"}),
    );
    assert_eq!(
        message(&e),
        "ConditionalOperator can only be used when Filter or Expected has two or more elements"
    );
}

/// Recorded 2026-11-19: these say it behind `1 validation error detected: `.
#[test]
fn some_legacy_refusals_are_counted_as_one_validation_error() {
    let mut w = world();
    let key = json!({"pk": {"S": "a"}, "sk": {"N": "1"}});
    let e = w.err(
        "PutItem",
        json!({"TableName": "tab", "Item": key, "Expected": {"n": {"Exists": true}}}),
    );
    assert_eq!(
        message(&e),
        "1 validation error detected: One or more parameter values were invalid: Value must be provided when Exists is true for Attribute: n"
    );
    let e = w.err(
        "PutItem",
        json!({"TableName": "tab", "Item": key, "Expected": {"n": {"Exists": false, "Value": {"N": "1"}}}}),
    );
    assert_eq!(
        message(&e),
        "1 validation error detected: One or more parameter values were invalid: Value cannot be used when Exists is false for Attribute: n"
    );
    let e = w.err(
        "UpdateItem",
        json!({"TableName": "tab", "Key": key, "AttributeUpdates": {"n": {"Action": "PUT"}}}),
    );
    assert_eq!(
        message(&e),
        "1 validation error detected: One or more parameter values were invalid: Only DELETE action is allowed when no attribute value is specified"
    );
    let e = w.err(
        "Scan",
        json!({"TableName": "tab", "AttributesToGet": ["n"], "Select": "ALL_ATTRIBUTES"}),
    );
    assert_eq!(
        message(&e),
        "1 validation error detected: Cannot specify the AttributesToGet when choosing to get ALL_ATTRIBUTES"
    );
}

#[test]
fn legacy_parameters_take_no_expression_placeholders() {
    let mut w = world();
    let e = w.err(
        "Scan",
        json!({"TableName": "tab", "ScanFilter": {"n": {"ComparisonOperator": "NULL"}}, "ExpressionAttributeNames": {"#n": "n"}}),
    );
    assert_eq!(
        message(&e),
        "ExpressionAttributeNames can only be specified when using expressions"
    );
}
