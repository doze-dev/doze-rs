//! Step Functions as an EventBridge target: a rule on the default bus starts an execution with the event as its input.

use serde_json::json;

use super::{ROLE, World, output};

#[tokio::test]
async fn an_eventbridge_rule_starts_an_execution() {
    let w = World::new();
    let m = w
        .machine(
            "ev",
            json!({"StartAt": "A", "States": {"A": {"Type": "Pass", "End": true}}}),
        )
        .await;
    w.peer(
        "events",
        "PutRule",
        json!({"Name": "to-sfn", "EventPattern": "{\"source\": [\"my.app\"]}"}),
    )
    .await;
    w.peer(
        "events",
        "PutTargets",
        json!({"Rule": "to-sfn", "Targets": [{"Id": "1", "Arn": m, "RoleArn": ROLE}]}),
    )
    .await;
    w.peer(
        "events",
        "PutEvents",
        json!({"Entries": [{"Source": "my.app", "DetailType": "t", "Detail": "{\"k\": 1}"}]}),
    )
    .await;
    for _ in 0..100 {
        let list = w.ok("ListExecutions", json!({"stateMachineArn": m})).await;
        if let Some(e) = list["executions"].as_array().and_then(|a| a.first()) {
            let d = w.finished(e["executionArn"].as_str().unwrap()).await;
            assert_eq!(output(&d)["detail"], json!({"k": 1}));
            w.stop().await;
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("the rule never started an execution");
}
