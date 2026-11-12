//! Query, scan, pagination and secondary indexes.

use serde_json::{Value, json};

use super::{World, message};

fn world() -> World {
    let mut w = World::new();
    w.table("tab2", true);
    for sk in 1..=5 {
        w.put(
            "tab2",
            json!({"pk": {"S": "a"}, "sk": {"N": sk.to_string()}, "even": {"BOOL": sk % 2 == 0}}),
        );
    }
    w.put("tab2", json!({"pk": {"S": "b"}, "sk": {"N": "1"}}));
    w
}

fn q(extra: Value) -> Value {
    let mut r = json!({"TableName": "tab2", "KeyConditionExpression": "pk = :p", "ExpressionAttributeValues": {":p": {"S": "a"}}});
    for (k, v) in extra.as_object().unwrap() {
        if k == "ExpressionAttributeValues" {
            for (a, b) in v.as_object().unwrap() {
                r["ExpressionAttributeValues"][a] = b.clone();
            }
        } else {
            r[k] = v.clone();
        }
    }
    r
}

fn sks(out: &Value) -> Vec<&str> {
    out["Items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["sk"]["N"].as_str().unwrap())
        .collect()
}

#[test]
fn the_recorded_query_shapes() {
    let mut w = world();
    let all = w.ok("Query", q(json!({"ConsistentRead": true})));
    assert_eq!(
        (sks(&all), all["Count"].clone(), all["ScannedCount"].clone()),
        (vec!["1", "2", "3", "4", "5"], json!(5), json!(5))
    );
    assert!(all.get("LastEvaluatedKey").is_none());
    let desc = w.ok("Query", q(json!({"ScanIndexForward": false, "Limit": 2})));
    assert_eq!(sks(&desc), ["5", "4"]);
    assert_eq!(
        desc["LastEvaluatedKey"],
        json!({"pk": {"S": "a"}, "sk": {"N": "4"}})
    );
    let page1 = w.ok("Query", q(json!({"Limit": 2})));
    let page2 = w.ok(
        "Query",
        q(json!({"Limit": 2, "ExclusiveStartKey": page1["LastEvaluatedKey"]})),
    );
    assert_eq!((sks(&page1), sks(&page2)), (vec!["1", "2"], vec!["3", "4"]));
    let between = w.ok(
        "Query",
        json!({"TableName": "tab2", "KeyConditionExpression": "pk = :p AND sk BETWEEN :lo AND :hi",
            "ExpressionAttributeValues": {":p": {"S": "a"}, ":lo": {"N": "2"}, ":hi": {"N": "4"}}}),
    );
    assert_eq!(sks(&between), ["2", "3", "4"]);
    let filtered = w.ok("Query", q(json!({"FilterExpression": "even = :t", "ExpressionAttributeValues": {":t": {"BOOL": true}}, "Limit": 3})));
    assert_eq!(
        (
            sks(&filtered),
            filtered["Count"].clone(),
            filtered["ScannedCount"].clone()
        ),
        (vec!["2"], json!(1), json!(3))
    );
    assert_eq!(
        filtered["LastEvaluatedKey"],
        json!({"pk": {"S": "a"}, "sk": {"N": "3"}})
    );
    let projected = w.ok(
        "Query",
        q(json!({"ProjectionExpression": "sk", "Limit": 1})),
    );
    assert_eq!(projected["Items"], json!([{"sk": {"N": "1"}}]));
    assert_eq!(
        projected["LastEvaluatedKey"],
        json!({"pk": {"S": "a"}, "sk": {"N": "1"}})
    );
    let count = w.ok("Query", q(json!({"Select": "COUNT"})));
    assert_eq!(
        (count["Count"].clone(), count["ScannedCount"].clone()),
        (json!(5), json!(5))
    );
    assert!(count.get("Items").is_none());
}

#[test]
fn sort_key_operators() {
    let mut w = world();
    let with = |expr: &str, v: Value| json!({"TableName": "tab2", "KeyConditionExpression": expr, "ExpressionAttributeValues": v});
    let p = json!({":p": {"S": "a"}, ":v": {"N": "3"}});
    for (expr, want) in [
        ("pk = :p AND sk = :v", vec!["3"]),
        ("pk = :p AND sk < :v", vec!["1", "2"]),
        ("pk = :p AND sk <= :v", vec!["1", "2", "3"]),
        ("pk = :p AND sk > :v", vec!["4", "5"]),
        ("pk = :p AND sk >= :v", vec!["3", "4", "5"]),
    ] {
        assert_eq!(sks(&w.ok("Query", with(expr, p.clone()))), want, "{expr}");
        let mut desc = with(expr, p.clone());
        desc["ScanIndexForward"] = json!(false);
        let mut rev = want.clone();
        rev.reverse();
        assert_eq!(sks(&w.ok("Query", desc)), rev, "{expr} descending");
    }
}

#[test]
fn string_sort_keys_and_begins_with() {
    let mut w = World::new();
    w.ok(
        "CreateTable",
        json!({"TableName": "names", "BillingMode": "PAY_PER_REQUEST",
            "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}, {"AttributeName": "sk", "KeyType": "RANGE"}],
            "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}, {"AttributeName": "sk", "AttributeType": "S"}]}),
    );
    for sk in ["ab", "abc", "abd", "b", "a"] {
        w.put("names", json!({"pk": {"S": "x"}, "sk": {"S": sk}}));
    }
    let out = w.ok(
        "Query",
        json!({"TableName": "names", "KeyConditionExpression": "pk = :p AND begins_with(sk, :s)",
            "ExpressionAttributeValues": {":p": {"S": "x"}, ":s": {"S": "ab"}}}),
    );
    let got: Vec<&str> = out["Items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["sk"]["S"].as_str().unwrap())
        .collect();
    assert_eq!(got, ["ab", "abc", "abd"]);
}

#[test]
fn query_refusals() {
    let mut w = world();
    let e = w.err("Query", json!({"TableName": "tab2"}));
    assert!(message(&e).starts_with("Either the KeyConditions or KeyConditionExpression"));
    let e = w.err("Query", json!({"TableName": "tab2", "KeyConditionExpression": "sk = :v", "ExpressionAttributeValues": {":v": {"N": "1"}}}));
    assert_eq!(message(&e), "Query condition missed key schema element: pk");
    let e = w.err(
        "Query",
        q(json!({"FilterExpression": "sk > :s", "ExpressionAttributeValues": {":s": {"N": "1"}}})),
    );
    assert_eq!(
        message(&e),
        "Filter Expression can only contain non-primary key attributes: Primary key attribute: sk"
    );
    let e = w.err(
        "Query",
        q(json!({"ExclusiveStartKey": {"pk": {"S": "b"}, "sk": {"N": "1"}}})),
    );
    assert!(message(&e).contains("outside query boundaries"));
    let e = w.err("Query", q(json!({"ExclusiveStartKey": {"pk": {"S": "a"}}})));
    assert_eq!(message(&e), "The provided starting key is invalid");
    let e = w.err("Query", q(json!({"IndexName": "nope"})));
    assert_eq!(
        message(&e),
        "The table does not have the specified index: nope"
    );
    let e = w.err("Query", q(json!({"Select": "ALL_PROJECTED_ATTRIBUTES"})));
    assert!(message(&e).contains("only when Querying using an IndexName"));
    let e = w.err("Query", json!({"TableName": "tab2", "KeyConditionExpression": "pk = :p", "ExpressionAttributeValues": {":p": {"S": "a"}}, "ExpressionAttributeNames": {"#x": "y"}}));
    assert!(
        message(&e).contains(
            "Value provided in ExpressionAttributeNames unused in expressions: keys: {#x}"
        )
    );
}

#[test]
fn scan_pages_filters_and_segments() {
    let mut w = world();
    let all = w.ok("Scan", json!({"TableName": "tab2", "ConsistentRead": true}));
    assert_eq!(
        (all["Count"].clone(), all["ScannedCount"].clone()),
        (json!(6), json!(6))
    );
    let mut seen = Vec::new();
    let mut start: Option<Value> = None;
    loop {
        let mut req = json!({"TableName": "tab2", "Limit": 4});
        if let Some(s) = &start {
            req["ExclusiveStartKey"] = s.clone();
        }
        let page = w.ok("Scan", req);
        seen.extend(page["Items"].as_array().unwrap().clone());
        match page.get("LastEvaluatedKey") {
            Some(k) => start = Some(k.clone()),
            None => break,
        }
    }
    assert_eq!(seen.len(), 6);
    let evens = w.ok("Scan", json!({"TableName": "tab2", "FilterExpression": "even = :t", "ExpressionAttributeValues": {":t": {"BOOL": true}}}));
    assert_eq!(
        (evens["Count"].clone(), evens["ScannedCount"].clone()),
        (json!(2), json!(6))
    );
    let mut total = 0;
    for segment in 0..3 {
        total += w.ok(
            "Scan",
            json!({"TableName": "tab2", "Segment": segment, "TotalSegments": 3}),
        )["Count"]
            .as_i64()
            .unwrap();
    }
    assert_eq!(total, 6);
    let e = w.err(
        "Scan",
        json!({"TableName": "tab2", "Segment": 3, "TotalSegments": 3}),
    );
    assert!(message(&e).contains("zero-based"));
    let e = w.err("Scan", json!({"TableName": "tab2", "Segment": 1}));
    assert!(message(&e).contains("TotalSegments parameter is required"));
}

#[test]
fn a_page_ends_at_one_megabyte() {
    let mut w = World::new();
    w.table("big", false);
    let blob = "x".repeat(300 * 1024);
    for i in 0..5 {
        w.put(
            "big",
            json!({"pk": {"S": format!("k{i}")}, "blob": {"S": blob}}),
        );
    }
    let page = w.ok("Scan", json!({"TableName": "big"}));
    assert_eq!(page["Count"], 4);
    assert!(page.get("LastEvaluatedKey").is_some());
}

fn indexed() -> World {
    let mut w = World::new();
    w.ok(
        "CreateTable",
        json!({"TableName": "docs", "BillingMode": "PAY_PER_REQUEST",
            "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
            "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}, {"AttributeName": "owner", "AttributeType": "S"}, {"AttributeName": "at", "AttributeType": "N"}],
            "GlobalSecondaryIndexes": [{"IndexName": "by-owner", "KeySchema": [{"AttributeName": "owner", "KeyType": "HASH"}, {"AttributeName": "at", "KeyType": "RANGE"}],
                "Projection": {"ProjectionType": "INCLUDE", "NonKeyAttributes": ["title"]}}]}),
    );
    for (i, owner) in ["ada", "ada", "bob"].iter().enumerate() {
        w.put("docs", json!({"pk": {"S": format!("doc-{i}")}, "owner": {"S": owner}, "at": {"N": i.to_string()}, "title": {"S": format!("t{i}")}, "body": {"S": "not projected"}}));
    }
    w.put("docs", json!({"pk": {"S": "no-owner"}}));
    w
}

#[test]
fn a_global_index_holds_the_projection_and_is_sparse() {
    let mut w = indexed();
    let by = |extra: Value| {
        let mut r = json!({"TableName": "docs", "IndexName": "by-owner", "KeyConditionExpression": "#o = :o",
            "ExpressionAttributeNames": {"#o": "owner"}, "ExpressionAttributeValues": {":o": {"S": "ada"}}});
        for (k, v) in extra.as_object().unwrap() {
            r[k] = v.clone();
        }
        r
    };
    let out = w.ok("Query", by(json!({})));
    assert_eq!(out["Count"], 2);
    assert_eq!(
        out["Items"][0],
        json!({"at": {"N": "0"}, "owner": {"S": "ada"}, "pk": {"S": "doc-0"}, "title": {"S": "t0"}})
    );
    let newest = w.ok("Query", by(json!({"ScanIndexForward": false, "Limit": 1})));
    assert_eq!(newest["Items"][0]["pk"], json!({"S": "doc-1"}));
    assert_eq!(
        newest["LastEvaluatedKey"],
        json!({"at": {"N": "1"}, "owner": {"S": "ada"}, "pk": {"S": "doc-1"}})
    );
    let next = w.ok("Query", by(json!({"ScanIndexForward": false, "Limit": 1, "ExclusiveStartKey": newest["LastEvaluatedKey"]})));
    assert_eq!(next["Items"][0]["pk"], json!({"S": "doc-0"}));
    assert_eq!(
        w.ok(
            "Scan",
            json!({"TableName": "docs", "IndexName": "by-owner", "Select": "COUNT"})
        )["Count"],
        3
    );
    assert_eq!(
        message(&w.err("Query", by(json!({"ConsistentRead": true})))),
        "Consistent reads are not supported on global secondary indexes"
    );
    assert_eq!(
        message(&w.err("Query", by(json!({"Select": "ALL_ATTRIBUTES"})))),
        "One or more parameter values were invalid: Select type ALL_ATTRIBUTES is not supported for global secondary index by-owner because its projection type is not ALL"
    );
    let e = w.err(
        "Query",
        json!({"TableName": "docs", "IndexName": "by-owner", "KeyConditionExpression": "owner = :o", "ExpressionAttributeValues": {":o": {"S": "ada"}}}),
    );
    assert_eq!(
        message(&e),
        "Invalid KeyConditionExpression: Attribute name is a reserved keyword; reserved keyword: owner"
    );
}

#[test]
fn an_index_follows_its_items() {
    let mut w = indexed();
    let count = |w: &mut World, who: &str| {
        w.ok(
            "Query",
            json!({"TableName": "docs", "IndexName": "by-owner", "KeyConditionExpression": "#o = :o",
                "ExpressionAttributeNames": {"#o": "owner"}, "ExpressionAttributeValues": {":o": {"S": who}}}),
        )["Count"]
            .as_i64()
            .unwrap()
    };
    assert_eq!((count(&mut w, "ada"), count(&mut w, "bob")), (2, 1));
    w.ok("UpdateItem", json!({"TableName": "docs", "Key": {"pk": {"S": "doc-0"}}, "UpdateExpression": "SET #o = :b", "ExpressionAttributeNames": {"#o": "owner"}, "ExpressionAttributeValues": {":b": {"S": "bob"}}}));
    assert_eq!((count(&mut w, "ada"), count(&mut w, "bob")), (1, 2));
    w.ok(
        "DeleteItem",
        json!({"TableName": "docs", "Key": {"pk": {"S": "doc-1"}}}),
    );
    assert_eq!(count(&mut w, "ada"), 0);
    let e = w.err(
        "PutItem",
        json!({"TableName": "docs", "Item": {"pk": {"S": "bad"}, "owner": {"N": "1"}}}),
    );
    assert_eq!(
        message(&e),
        "One or more parameter values were invalid: Type mismatch for Index Key owner Expected: S Actual: N IndexName: by-owner"
    );
}

#[test]
fn an_index_added_later_is_backfilled_and_can_be_removed() {
    let mut w = World::new();
    w.table("late", false);
    w.put("late", json!({"pk": {"S": "a"}, "color": {"S": "red"}}));
    w.put("late", json!({"pk": {"S": "b"}, "color": {"S": "blue"}}));
    w.put("late", json!({"pk": {"S": "c"}}));
    let up = w.ok(
        "UpdateTable",
        json!({"TableName": "late", "AttributeDefinitions": [{"AttributeName": "color", "AttributeType": "S"}],
            "GlobalSecondaryIndexUpdates": [{"Create": {"IndexName": "by-color", "KeySchema": [{"AttributeName": "color", "KeyType": "HASH"}], "Projection": {"ProjectionType": "ALL"}}}]}),
    );
    assert_eq!(
        up["TableDescription"]["GlobalSecondaryIndexes"][0]["IndexStatus"],
        "CREATING"
    );
    assert_eq!(
        up["TableDescription"]["GlobalSecondaryIndexes"][0]["Backfilling"],
        true
    );
    let red = |w: &mut World| {
        w.ok("Query", json!({"TableName": "late", "IndexName": "by-color", "KeyConditionExpression": "color = :c", "ExpressionAttributeValues": {":c": {"S": "red"}}}))
    };
    assert_eq!(red(&mut w)["Items"][0]["pk"], json!({"S": "a"}));
    let d = w.ok("DescribeTable", json!({"TableName": "late"}));
    assert_eq!(
        d["Table"]["GlobalSecondaryIndexes"][0]["IndexStatus"],
        "ACTIVE"
    );
    assert_eq!(d["Table"]["GlobalSecondaryIndexes"][0]["ItemCount"], 2);
    w.ok("UpdateTable", json!({"TableName": "late", "GlobalSecondaryIndexUpdates": [{"Delete": {"IndexName": "by-color"}}]}));
    let e = w.err("Query", json!({"TableName": "late", "IndexName": "by-color", "KeyConditionExpression": "color = :c", "ExpressionAttributeValues": {":c": {"S": "red"}}}));
    assert!(message(&e).contains("does not have the specified index"));
}

#[test]
fn a_local_index_fetches_the_whole_item() {
    let mut w = World::new();
    w.ok(
        "CreateTable",
        json!({"TableName": "lsi", "BillingMode": "PAY_PER_REQUEST",
            "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}, {"AttributeName": "sk", "KeyType": "RANGE"}],
            "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}, {"AttributeName": "sk", "AttributeType": "N"}, {"AttributeName": "alt", "AttributeType": "S"}],
            "LocalSecondaryIndexes": [{"IndexName": "by-alt", "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}, {"AttributeName": "alt", "KeyType": "RANGE"}], "Projection": {"ProjectionType": "KEYS_ONLY"}}]}),
    );
    w.put(
        "lsi",
        json!({"pk": {"S": "a"}, "sk": {"N": "1"}, "alt": {"S": "z"}, "extra": {"S": "e"}}),
    );
    w.put(
        "lsi",
        json!({"pk": {"S": "a"}, "sk": {"N": "2"}, "alt": {"S": "y"}, "extra": {"S": "f"}}),
    );
    let req = json!({"TableName": "lsi", "IndexName": "by-alt", "KeyConditionExpression": "pk = :p", "ExpressionAttributeValues": {":p": {"S": "a"}}});
    let all = w.ok("Query", req.clone());
    assert_eq!(all["Items"][0]["alt"], json!({"S": "y"}));
    assert_eq!(all["Items"][0]["extra"], json!({"S": "f"}));
    let mut projected = req;
    projected["Select"] = json!("ALL_PROJECTED_ATTRIBUTES");
    assert!(w.ok("Query", projected)["Items"][0].get("extra").is_none());
}
