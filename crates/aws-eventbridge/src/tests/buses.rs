//! Buses, rules, tags and permissions: the recorded answers (2026-11-09) and the rules around them.

use serde_json::json;

use super::{ACCOUNT, World};

const BUS_ARN: &str = "arn:aws:events:us-east-1:000000000000:event-bus";

#[test]
fn a_bus_is_made_described_and_deleted_as_aws_recorded() {
    let mut w = World::new();
    let made = w.ok("CreateEventBus", &json!({"Name": "shop"}));
    assert_eq!(made, json!({"EventBusArn": format!("{BUS_ARN}/shop")}));
    let d = w.ok("DescribeEventBus", &json!({"Name": "shop"}));
    assert_eq!(d["Name"], "shop");
    assert_eq!(d["Arn"], format!("{BUS_ARN}/shop"));
    assert!(d["CreationTime"].is_number() && d["LastModifiedTime"].is_number());
    assert!(d.get("Policy").is_none() && d.get("Description").is_none());
    assert_eq!(w.ok("DeleteEventBus", &json!({"Name": "shop"})), json!({}));
    // Deleting again is not an error (recorded).
    assert_eq!(w.ok("DeleteEventBus", &json!({"Name": "shop"})), json!({}));
}

#[test]
fn the_wording_of_the_recorded_bus_refusals() {
    let mut w = World::new();
    w.bus("shop");
    assert_eq!(
        w.err("CreateEventBus", &json!({"Name": "shop"})),
        (
            "ResourceAlreadyExistsException".into(),
            "Event bus shop already exists.".into()
        )
    );
    assert_eq!(
        w.err("DescribeEventBus", &json!({"Name": "absent"})),
        (
            "ResourceNotFoundException".into(),
            "Event bus absent does not exist.".into()
        )
    );
    assert_eq!(
        w.err(
            "PutRule",
            &json!({"Name": "r", "EventBusName": "absent", "EventPattern": "{\"source\":[\"x\"]}"})
        )
        .1,
        "Event bus absent does not exist."
    );
}

#[test]
fn every_account_has_a_default_bus_that_cannot_be_deleted() {
    let mut w = World::new();
    let d = w.ok("DescribeEventBus", &json!({}));
    assert_eq!(d["Name"], "default");
    assert_eq!(d["Arn"], format!("{BUS_ARN}/default"));
    let listed = w.ok("ListEventBuses", &json!({}));
    assert_eq!(listed["EventBuses"].as_array().unwrap().len(), 1);
    let (code, _) = w.err("DeleteEventBus", &json!({"Name": "default"}));
    assert_eq!(code, "ValidationException");
}

#[test]
fn a_bus_with_rules_is_not_deleted_and_the_rules_are_listed_by_prefix_and_page() {
    let mut w = World::new();
    w.bus("shop");
    for n in ["a-one", "a-two", "b-one"] {
        w.rule("shop", n, &json!({"source": ["x"]}));
    }
    assert_eq!(
        w.err("DeleteEventBus", &json!({"Name": "shop"})).0,
        "ValidationException"
    );
    let by_prefix = w.ok(
        "ListRules",
        &json!({"EventBusName": "shop", "NamePrefix": "a-"}),
    );
    assert_eq!(by_prefix["Rules"].as_array().unwrap().len(), 2);
    let first = w.ok("ListRules", &json!({"EventBusName": "shop", "Limit": 2}));
    assert_eq!(first["Rules"].as_array().unwrap().len(), 2);
    let second = w.ok(
        "ListRules",
        &json!({"EventBusName": "shop", "Limit": 2, "NextToken": first["NextToken"]}),
    );
    assert_eq!(second["Rules"].as_array().unwrap().len(), 1);
    assert_eq!(second["Rules"][0]["Name"], "b-one");
    assert!(second.get("NextToken").is_none());
}

#[test]
fn buses_are_kept_apart_by_account() {
    let mut w = World::new();
    w.bus("shop");
    let other = w.run("DescribeEventBus", &json!({"Name": "shop"}));
    assert!(other.is_ok());
    assert!(w.core.buses.get(&format!("{ACCOUNT}/shop")).is_some());
    assert!(w.core.buses.get("111122223333/shop").is_none());
}

#[test]
fn a_bus_can_be_described_by_arn_updated_and_tagged() {
    let mut w = World::new();
    let arn = w.ok(
        "CreateEventBus",
        &json!({"Name": "shop", "Description": "d", "Tags": [{"Key": "a", "Value": "1"}]}),
    )["EventBusArn"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        w.ok("DescribeEventBus", &json!({"Name": arn}))["Description"],
        "d"
    );
    w.ok("UpdateEventBus", &json!({"Name": "shop", "Description": "new", "DeadLetterConfig": {"Arn": "arn:aws:sqs:us-east-1:000000000000:dlq"}}));
    let d = w.ok("DescribeEventBus", &json!({"Name": "shop"}));
    assert_eq!(d["Description"], "new");
    assert_eq!(
        d["DeadLetterConfig"]["Arn"],
        "arn:aws:sqs:us-east-1:000000000000:dlq"
    );
    assert_eq!(
        w.ok("ListTagsForResource", &json!({"ResourceARN": arn}))["Tags"],
        json!([{"Key": "a", "Value": "1"}])
    );
    w.ok(
        "TagResource",
        &json!({"ResourceARN": arn, "Tags": [{"Key": "env", "Value": "dev"}]}),
    );
    w.ok(
        "UntagResource",
        &json!({"ResourceARN": arn, "TagKeys": ["a"]}),
    );
    assert_eq!(
        w.ok("ListTagsForResource", &json!({"ResourceARN": arn}))["Tags"],
        json!([{"Key": "env", "Value": "dev"}])
    );
}

#[test]
fn permissions_build_the_bus_policy_and_take_statements_away() {
    let mut w = World::new();
    w.bus("shop");
    w.ok("PutPermission", &json!({"EventBusName": "shop", "StatementId": "s1", "Action": "events:PutEvents", "Principal": "111122223333"}));
    w.ok("PutPermission", &json!({"EventBusName": "shop", "StatementId": "s2", "Action": "events:PutEvents", "Principal": "*",
        "Condition": {"Type": "StringEquals", "Key": "aws:PrincipalOrgID", "Value": "o-abc"}}));
    let policy: serde_json::Value = serde_json::from_str(
        w.ok("DescribeEventBus", &json!({"Name": "shop"}))["Policy"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let statements = policy["Statement"].as_array().unwrap();
    assert_eq!(statements.len(), 2);
    assert_eq!(
        statements[0]["Principal"],
        json!({"AWS": "arn:aws:iam::111122223333:root"})
    );
    assert_eq!(statements[0]["Resource"], format!("{BUS_ARN}/shop"));
    assert_eq!(statements[1]["Principal"], "*");
    assert_eq!(
        statements[1]["Condition"],
        json!({"StringEquals": {"aws:PrincipalOrgID": "o-abc"}})
    );
    assert_eq!(w.err("PutPermission", &json!({"EventBusName": "shop", "StatementId": "s1", "Action": "events:PutEvents", "Principal": "111122223333"})), ("ValidationException".into(), "Statement already exists.".into()));
    w.ok(
        "RemovePermission",
        &json!({"EventBusName": "shop", "StatementId": "s1"}),
    );
    assert_eq!(
        w.err(
            "RemovePermission",
            &json!({"EventBusName": "shop", "StatementId": "s1"})
        )
        .0,
        "ResourceNotFoundException"
    );
    w.ok(
        "RemovePermission",
        &json!({"EventBusName": "shop", "RemoveAllPermissions": true}),
    );
    assert!(
        w.ok("DescribeEventBus", &json!({"Name": "shop"}))
            .get("Policy")
            .is_none()
    );
    // A whole policy may be given instead.
    let whole = json!({"Version": "2012-10-17", "Statement": []}).to_string();
    w.ok(
        "PutPermission",
        &json!({"EventBusName": "shop", "Policy": whole}),
    );
    assert_eq!(
        w.ok("DescribeEventBus", &json!({"Name": "shop"}))["Policy"],
        whole
    );
}
