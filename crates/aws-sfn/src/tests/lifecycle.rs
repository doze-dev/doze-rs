//! The control plane: machines, versions, aliases, activities, tags, validation.

use serde_json::json;

use super::{ROLE, World};

fn pass() -> serde_json::Value {
    json!({"StartAt": "A", "States": {"A": {"Type": "Pass", "End": true}}})
}

#[tokio::test]
async fn machines_are_made_described_updated_listed_and_deleted() {
    let w = World::new();
    let arn = w.machine("m", pass()).await;
    let again = w
        .ok(
            "CreateStateMachine",
            json!({"name": "m", "definition": pass().to_string(), "roleArn": ROLE}),
        )
        .await;
    assert_eq!(again["stateMachineArn"], arn);
    let other = json!({"StartAt": "B", "States": {"B": {"Type": "Succeed"}}});
    let conflict = w
        .call(
            "CreateStateMachine",
            json!({"name": "m", "definition": other.to_string(), "roleArn": ROLE}),
        )
        .await
        .unwrap_err();
    assert_eq!(conflict.code, "StateMachineAlreadyExists");
    let d = w
        .ok("DescribeStateMachine", json!({"stateMachineArn": arn}))
        .await;
    assert_eq!(
        d["loggingConfiguration"],
        json!({"level": "OFF", "includeExecutionData": false})
    );
    assert_eq!(
        d["encryptionConfiguration"],
        json!({"type": "AWS_OWNED_KEY"})
    );
    let updated = w
        .ok(
            "UpdateStateMachine",
            json!({"stateMachineArn": arn, "definition": other.to_string(), "publish": true}),
        )
        .await;
    assert_eq!(updated["stateMachineVersionArn"], format!("{arn}:1"));
    let listed = w.ok("ListStateMachines", json!({})).await;
    assert_eq!(listed["stateMachines"].as_array().unwrap().len(), 1);
    w.ok("DeleteStateMachine", json!({"stateMachineArn": arn}))
        .await;
    w.ok("DeleteStateMachine", json!({"stateMachineArn": arn}))
        .await;
    let gone = w
        .call("DescribeStateMachine", json!({"stateMachineArn": arn}))
        .await
        .unwrap_err();
    assert_eq!(gone.code, "StateMachineDoesNotExist");
    w.stop().await;
}

#[tokio::test]
async fn versions_and_aliases_route_executions() {
    let w = World::new();
    let arn = w.machine("v", pass()).await;
    let v1 = w
        .ok(
            "PublishStateMachineVersion",
            json!({"stateMachineArn": arn}),
        )
        .await;
    let v1 = v1["stateMachineVersionArn"].as_str().unwrap().to_owned();
    let new =
        json!({"StartAt": "A", "States": {"A": {"Type": "Pass", "Result": "two", "End": true}}});
    w.ok(
        "UpdateStateMachine",
        json!({"stateMachineArn": arn, "definition": new.to_string()}),
    )
    .await;
    let v2 = w
        .ok(
            "PublishStateMachineVersion",
            json!({"stateMachineArn": arn, "description": "second"}),
        )
        .await;
    let v2 = v2["stateMachineVersionArn"].as_str().unwrap().to_owned();
    let alias = w
        .ok(
            "CreateStateMachineAlias",
            json!({"name": "prod", "routingConfiguration": [{"stateMachineVersionArn": v1, "weight": 100}]}),
        )
        .await;
    let alias = alias["stateMachineAliasArn"].as_str().unwrap().to_owned();
    let d = w.run(&alias, json!({"x": 1})).await;
    assert_eq!(super::output(&d), json!({"x": 1}));
    assert_eq!(d["stateMachineAliasArn"], alias);
    assert_eq!(d["stateMachineVersionArn"], v1);
    let busy = w
        .call(
            "DeleteStateMachineVersion",
            json!({"stateMachineVersionArn": v1}),
        )
        .await
        .unwrap_err();
    assert_eq!(busy.code, "ConflictException");
    repoint(&w, &arn, &alias, &v1, &v2).await;
    w.stop().await;
}

/// The alias moved to version 2, a bad weight refused, and the versions listed and removed.
async fn repoint(w: &World, arn: &str, alias: &str, v1: &str, v2: &str) {
    w.ok(
        "UpdateStateMachineAlias",
        json!({"stateMachineAliasArn": alias, "routingConfiguration": [{"stateMachineVersionArn": v2, "weight": 100}]}),
    )
    .await;
    assert_eq!(super::output(&w.run(alias, json!({})).await), json!("two"));
    let bad = w
        .call(
            "UpdateStateMachineAlias",
            json!({"stateMachineAliasArn": alias, "routingConfiguration": [{"stateMachineVersionArn": v2, "weight": 50}]}),
        )
        .await
        .unwrap_err();
    assert_eq!(bad.code, "ValidationException");
    let versions = w
        .ok("ListStateMachineVersions", json!({"stateMachineArn": arn}))
        .await;
    assert_eq!(
        versions["stateMachineVersions"][0]["stateMachineVersionArn"],
        v2
    );
    let described = w
        .ok("DescribeStateMachine", json!({"stateMachineArn": v2}))
        .await;
    assert_eq!(described["description"], "second");
    w.ok(
        "DeleteStateMachineAlias",
        json!({"stateMachineAliasArn": alias}),
    )
    .await;
    w.ok(
        "DeleteStateMachineVersion",
        json!({"stateMachineVersionArn": v1}),
    )
    .await;
}

#[tokio::test]
async fn activities_and_tags() {
    let w = World::new();
    let a = w
        .ok(
            "CreateActivity",
            json!({"name": "work", "tags": [{"key": "team", "value": "x"}]}),
        )
        .await;
    let a = a["activityArn"].as_str().unwrap().to_owned();
    assert_eq!(
        w.ok("DescribeActivity", json!({"activityArn": a})).await["name"],
        "work"
    );
    let tags = w.ok("ListTagsForResource", json!({"resourceArn": a})).await;
    assert_eq!(tags["tags"], json!([{"key": "team", "value": "x"}]));
    let m = w.machine("t", pass()).await;
    w.ok(
        "TagResource",
        json!({"resourceArn": m, "tags": [{"key": "k", "value": "v"}]}),
    )
    .await;
    w.ok("UntagResource", json!({"resourceArn": m, "tagKeys": ["k"]}))
        .await;
    assert_eq!(
        w.ok("ListTagsForResource", json!({"resourceArn": m})).await["tags"],
        json!([])
    );
    let absent = w
        .call(
            "ListTagsForResource",
            json!({"resourceArn": format!("{m}-nope")}),
        )
        .await
        .unwrap_err();
    assert_eq!(absent.code, "ResourceNotFound");
    assert_eq!(
        w.ok("ListActivities", json!({})).await["activities"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    w.ok("DeleteActivity", json!({"activityArn": a})).await;
    w.stop().await;
}

#[tokio::test]
async fn validation_lists_every_problem() {
    let w = World::new();
    let bad = json!({"StartAt": "Z", "States": {"A": {"Type": "Pass", "Next": "Nowhere"}}});
    let v = w
        .ok(
            "ValidateStateMachineDefinition",
            json!({"definition": bad.to_string()}),
        )
        .await;
    assert_eq!(v["result"], "FAIL");
    let codes: Vec<&str> = v["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect();
    assert_eq!(
        codes,
        [
            "MISSING_TRANSITION_TARGET",
            "MISSING_TRANSITION_TARGET",
            "MISSING_TRANSITION_TARGET",
            "MISSING_END_STATE"
        ]
    );
    let ok = w
        .ok(
            "ValidateStateMachineDefinition",
            json!({"definition": pass().to_string()}),
        )
        .await;
    assert_eq!(ok["result"], "OK");
    let express_token = json!({"StartAt": "T", "States": {"T": {"Type": "Task",
        "Resource": "arn:aws:states:::sqs:sendMessage.waitForTaskToken", "End": true}}});
    let refused = w
        .call(
            "CreateStateMachine",
            json!({"name": "e", "definition": express_token.to_string(), "roleArn": ROLE, "type": "EXPRESS"}),
        )
        .await
        .unwrap_err();
    assert_eq!(refused.code, "InvalidDefinition");
    w.stop().await;
}
