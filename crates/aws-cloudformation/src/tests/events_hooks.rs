//! DescribeEvents by operation, stack and change set, the hook reads and the account limits.

use serde_json::{Value, json};

use super::World;

fn queue(name: &str) -> Value {
    json!({"Type": "AWS::SQS::Queue", "Properties": {"QueueName": name}})
}

#[tokio::test]
async fn describe_events_by_operation_stack_and_change_set() {
    let w = World::new();
    let made = w
        .cfn("CreateStack", json!({"StackName": "ev", "TemplateBody": json!({"Resources": {"Q": queue("ev-q")}}).to_string()}))
        .await
        .unwrap();
    let op = made["OperationId"].as_str().unwrap().to_owned();
    assert_eq!(w.settle("ev").await, "CREATE_COMPLETE");
    let got = w
        .cfn("DescribeEvents", json!({"OperationId": op}))
        .await
        .unwrap();
    let events = got["OperationEvents"].as_array().unwrap();
    assert!(events.len() >= 6, "{events:?}");
    // Recorded 2026-11-19: the operation's own events carry its type and status and no resource; the stack events are PROGRESS
    // and carry neither.
    let ops: Vec<(&Value, &Value)> = events
        .iter()
        .filter(|e| e["EventType"] == "STACK_EVENT")
        .map(|e| (&e["OperationType"], &e["OperationStatus"]))
        .collect();
    assert_eq!(
        ops,
        [
            (&json!("CREATE_STACK"), &json!("SUCCEEDED")),
            (&json!("CREATE_STACK"), &json!("IN_PROGRESS")),
        ]
    );
    assert!(
        events
            .iter()
            .filter(|e| e["EventType"] == "STACK_EVENT")
            .all(|e| e.get("LogicalResourceId").is_none() && e.get("ResourceStatus").is_none())
    );
    assert!(
        events
            .iter()
            .filter(|e| e["EventType"] != "STACK_EVENT")
            .all(|e| e["EventType"] == "PROGRESS"
                && e.get("OperationType").is_none()
                && e.get("OperationStatus").is_none())
    );
    assert_eq!(events[0]["EventType"], "STACK_EVENT");
    assert!(events[0]["EndTime"].is_string());
    assert_eq!(events[1]["LogicalResourceId"], "ev");
    assert_eq!(events[1]["ResourceStatus"], "CREATE_COMPLETE");
    assert!(
        events
            .iter()
            .any(|e| e["LogicalResourceId"] == "Q" && e["EndTime"].is_string())
    );

    let body = json!({"Resources": {"Q": queue("ev-q"), "Bad": {"Type": "AWS::SSM::Parameter",
        "Properties": {"Name": "has space", "Type": "String", "Value": "v"}}}});
    w.cfn(
        "CreateChangeSet",
        json!({"StackName": "ev", "ChangeSetName": "cs", "TemplateBody": body.to_string()}),
    )
    .await
    .unwrap();
    w.cfn(
        "ExecuteChangeSet",
        json!({"StackName": "ev", "ChangeSetName": "cs"}),
    )
    .await
    .unwrap();
    assert_eq!(w.settle("ev").await, "UPDATE_ROLLBACK_COMPLETE");
    let by_cs = w
        .cfn(
            "DescribeEvents",
            json!({"StackName": "ev", "ChangeSetName": "cs"}),
        )
        .await
        .unwrap();
    let cs_events = by_cs["OperationEvents"].as_array().unwrap();
    let cs_ops: Vec<&Value> = cs_events
        .iter()
        .filter(|e| e["EventType"] == "STACK_EVENT")
        .map(|e| &e["OperationStatus"])
        .collect();
    assert_eq!(cs_ops, [&json!("FAILED"), &json!("IN_PROGRESS")]);
    assert!(
        cs_events
            .iter()
            .filter(|e| e["EventType"] == "STACK_EVENT")
            .all(|e| e["OperationType"] == "UPDATE_STACK")
    );
    let failed = w
        .cfn(
            "DescribeEvents",
            json!({"StackName": "ev", "Filters": {"FailedEvents": true}}),
        )
        .await
        .unwrap();
    let failed = failed["OperationEvents"].as_array().unwrap();
    assert!(!failed.is_empty());
    assert!(
        failed
            .iter()
            .all(|e| e["ResourceStatus"].as_str().unwrap().ends_with("FAILED"))
    );
    assert_eq!(failed[0]["EventType"], "PROVISIONING_ERROR");
    let all = w
        .cfn("DescribeEvents", json!({"StackName": "ev"}))
        .await
        .unwrap();
    assert!(all["OperationEvents"].as_array().unwrap().len() > events.len() + cs_events.len() - 1);
    let none = w.cfn("DescribeEvents", json!({})).await.unwrap_err();
    assert_eq!(none.code, "ValidationError");
}

#[tokio::test]
async fn hooks_are_none_and_limits_are_aws_defaults() {
    let w = World::new();
    assert_eq!(
        w.create("hk", json!({"Resources": {"Q": queue("hk-q")}}), &[])
            .await,
        "CREATE_COMPLETE"
    );
    let cs = w
        .cfn("CreateChangeSet", json!({"StackName": "hk", "ChangeSetName": "c1",
            "TemplateBody": json!({"Resources": {"Q": queue("hk-q"), "R": queue("hk-r")}}).to_string()}))
        .await
        .unwrap();
    let id = cs["Id"].as_str().unwrap().to_owned();
    let hooks = w
        .cfn(
            "DescribeChangeSetHooks",
            json!({"StackName": "hk", "ChangeSetName": "c1"}),
        )
        .await
        .unwrap();
    assert_eq!(hooks["Hooks"], json!([]));
    assert_eq!(hooks["Status"], "PLANNED");
    assert_eq!(hooks["ChangeSetId"], id.as_str());
    let absent = w
        .cfn(
            "DescribeChangeSetHooks",
            json!({"StackName": "hk", "ChangeSetName": "nope"}),
        )
        .await
        .unwrap_err();
    assert_eq!(absent.code, "ChangeSetNotFoundException");
    let all = w.cfn("ListHookResults", json!({})).await.unwrap();
    assert_eq!(all["HookResults"], json!([]));
    let of_cs = w
        .cfn(
            "ListHookResults",
            json!({"TargetType": "CHANGE_SET", "TargetId": id}),
        )
        .await
        .unwrap();
    assert_eq!(of_cs["HookResults"], json!([]));
    let half = w
        .cfn("ListHookResults", json!({"TargetType": "CHANGE_SET"}))
        .await
        .unwrap_err();
    assert_eq!(half.code, "ValidationError");
    let unknown = w
        .cfn(
            "ListHookResults",
            json!({"TargetType": "CLOUD_CONTROL", "TargetId": "token"}),
        )
        .await
        .unwrap_err();
    assert_eq!(unknown.code, "HookResultNotFoundException");
    let one = w
        .cfn(
            "GetHookResult",
            json!({"HookResultId": "00000000-0000-0000-0000-000000000000"}),
        )
        .await
        .unwrap_err();
    assert_eq!(one.code, "HookResultNotFoundException");
    assert_eq!(one.message, "The hook result was not found.");
    let limits = w.cfn("DescribeAccountLimits", json!({})).await.unwrap();
    assert_eq!(
        limits["AccountLimits"],
        json!([{"Name": "StackLimit", "Value": 2000}, {"Name": "StackOutputsLimit", "Value": 200},
            {"Name": "ConcurrentResourcesLimit", "Value": 2500}])
    );
}
