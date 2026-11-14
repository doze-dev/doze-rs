//! Executions: waits without a tick, a restart that carries on, stop, redrive, Express, Map Runs and `TestState`.

use std::time::Duration;

use serde_json::json;

use super::{World, output};

#[tokio::test]
async fn a_wait_sleeps_to_its_deadline_and_nothing_ticks_after() {
    let w = World::new();
    let m = w
        .machine(
            "w",
            json!({"StartAt": "W", "States": {"W": {"Type": "Wait", "Seconds": 1, "Next": "D"}, "D": {"Type": "Succeed"}}}),
        )
        .await;
    let e = w.start(&m, json!({"k": 1})).await;
    assert!(w.sfn.shared.timers.armed("us-east-1"));
    let d = w.finished(&e).await;
    assert_eq!(d["status"], "SUCCEEDED");
    assert_eq!(output(&d), json!({"k": 1}));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !w.sfn.shared.timers.armed("us-east-1"),
        "a timer is left with nothing due"
    );
    assert_eq!(
        w.history(&e).await,
        [
            "ExecutionStarted",
            "WaitStateEntered",
            "WaitStateExited",
            "SucceedStateEntered",
            "SucceedStateExited",
            "ExecutionSucceeded"
        ]
    );
    w.stop().await;
}

#[tokio::test]
async fn a_running_wait_carries_on_after_a_restart() {
    let w = World::new();
    let m = w
        .machine(
            "r",
            json!({"StartAt": "W", "States": {"W": {"Type": "Wait", "Seconds": 2, "End": true}}}),
        )
        .await;
    let e = w.start(&m, json!({"before": "restart"})).await;
    let dir = w.stop().await;
    let w = World::at(dir);
    w.sfn.resume().await;
    let d = w.finished(&e).await;
    assert_eq!(d["status"], "SUCCEEDED");
    assert_eq!(output(&d), json!({"before": "restart"}));
    w.stop().await;
}

#[tokio::test]
async fn stop_and_redrive() {
    let w = World::new();
    let m = w
        .machine(
            "s",
            json!({"StartAt": "W", "States": {"W": {"Type": "Wait", "Seconds": 30, "Next": "Done"}, "Done": {"Type": "Pass", "End": true}}}),
        )
        .await;
    let e = w.start(&m, json!({})).await;
    w.ok(
        "StopExecution",
        json!({"executionArn": e, "error": "Halt", "cause": "test"}),
    )
    .await;
    let d = w.ok("DescribeExecution", json!({"executionArn": e})).await;
    assert_eq!(
        (d["status"].as_str(), d["error"].as_str()),
        (Some("ABORTED"), Some("Halt"))
    );
    assert_eq!(d["redriveStatus"], "REDRIVABLE");
    assert!(w.history(&e).await.contains(&"ExecutionAborted".to_owned()));
    w.ok("RedriveExecution", json!({"executionArn": e})).await;
    let again = w.ok("DescribeExecution", json!({"executionArn": e})).await;
    assert_eq!(again["status"], "RUNNING");
    assert_eq!(again["redriveCount"], 1);
    let ok = w
        .machine(
            "ok",
            json!({"StartAt": "A", "States": {"A": {"Type": "Succeed"}}}),
        )
        .await;
    let done = w.run(&ok, json!({})).await;
    let refused = w
        .call(
            "RedriveExecution",
            json!({"executionArn": done["executionArn"]}),
        )
        .await
        .unwrap_err();
    assert_eq!(refused.code, "ExecutionNotRedrivable");
    w.stop().await;
}

#[tokio::test]
async fn express_runs_inside_the_call_and_leaves_nothing_to_describe() {
    let w = World::new();
    let def = json!({"StartAt": "W", "States": {"W": {"Type": "Wait", "Seconds": 1, "Next": "P"},
        "P": {"Type": "Pass", "Parameters": {"got.$": "$.v"}, "End": true}}});
    let m = w.machine_of("x", &def, true).await;
    let d = w
        .ok(
            "StartSyncExecution",
            json!({"stateMachineArn": m, "input": "{\"v\": 3}"}),
        )
        .await;
    assert_eq!(d["status"], "SUCCEEDED");
    assert_eq!(output(&d), json!({"got": 3}));
    assert!(d["executionArn"].as_str().unwrap().contains(":express:"));
    let not_here = w
        .call(
            "DescribeExecution",
            json!({"executionArn": d["executionArn"]}),
        )
        .await
        .unwrap_err();
    assert_eq!(not_here.code, "InvalidArn");
    let standard = w
        .machine(
            "st",
            json!({"StartAt": "A", "States": {"A": {"Type": "Succeed"}}}),
        )
        .await;
    let refused = w
        .call("StartSyncExecution", json!({"stateMachineArn": standard}))
        .await
        .unwrap_err();
    assert_eq!(refused.code, "StateMachineTypeNotSupported");
    w.stop().await;
}

#[tokio::test]
async fn a_distributed_map_has_a_map_run() {
    let w = World::new();
    let m = w
        .machine(
            "dm",
            json!({"StartAt": "M", "States": {"M": {"Type": "Map", "MaxConcurrency": 2, "ToleratedFailureCount": 1,
                "ItemProcessor": {"ProcessorConfig": {"Mode": "DISTRIBUTED", "ExecutionType": "STANDARD"}, "StartAt": "C", "States": {
                    "C": {"Type": "Choice", "Choices": [{"Variable": "$", "NumericEquals": 2, "Next": "Boom"}], "Default": "Double"},
                    "Boom": {"Type": "Fail", "Error": "Two"},
                    "Double": {"Type": "Pass", "End": true}}},
                "End": true}}}),
        )
        .await;
    let d = w.run(&m, json!([1, 2, 3])).await;
    assert_eq!(d["status"], "SUCCEEDED", "{d}");
    let runs = w
        .ok("ListMapRuns", json!({"executionArn": d["executionArn"]}))
        .await;
    let run = runs["mapRuns"][0]["mapRunArn"].as_str().unwrap().to_owned();
    let described = w.ok("DescribeMapRun", json!({"mapRunArn": run})).await;
    assert_eq!(described["status"], "SUCCEEDED");
    assert_eq!(described["maxConcurrency"], 2);
    assert_eq!(described["itemCounts"]["succeeded"], 2);
    let items = w.ok("ListExecutions", json!({"mapRunArn": run})).await;
    assert_eq!(items["executions"].as_array().unwrap().len(), 3, "{items}");
    let own = w.ok("ListExecutions", json!({"stateMachineArn": m})).await;
    assert_eq!(own["executions"].as_array().unwrap().len(), 1);
    assert_eq!(described["itemCounts"]["failed"], 1);
    w.stop().await;
}

#[tokio::test]
async fn test_state_runs_one_state() {
    let w = World::new();
    let choice = json!({"Type": "Choice", "Choices": [{"Variable": "$.n", "NumericGreaterThan": 1, "Next": "Big"}], "Default": "Small"});
    let t = w
        .ok("TestState", json!({"definition": choice.to_string(), "input": "{\"n\": 5}", "inspectionLevel": "DEBUG"}))
        .await;
    assert_eq!(
        (t["status"].as_str(), t["nextState"].as_str()),
        (Some("SUCCEEDED"), Some("Big"))
    );
    let task = json!({"Type": "Task", "Resource": "arn:aws:states:::lambda:invoke", "ResultPath": "$.r",
        "Retry": [{"ErrorEquals": ["Oops"], "MaxAttempts": 1}], "Next": "After"});
    let mocked = w
        .ok(
            "TestState",
            json!({"definition": task.to_string(), "input": "{}", "mock": {"result": "{\"ok\": 1}"},
            "inspectionLevel": "DEBUG"}),
        )
        .await;
    assert_eq!(mocked["status"], "SUCCEEDED");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(mocked["output"].as_str().unwrap()).unwrap(),
        json!({"r": {"ok": 1}})
    );
    assert!(mocked["inspectionData"]["afterResultPath"].is_string());
    let retried = w
        .ok("TestState", json!({"definition": task.to_string(), "mock": {"errorOutput": {"error": "Oops", "cause": "c"}}}))
        .await;
    assert_eq!(retried["status"], "RETRIABLE");
    assert_eq!(retried["error"], "Oops");
    w.stop().await;
}
