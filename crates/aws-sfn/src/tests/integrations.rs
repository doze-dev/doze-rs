//! Tasks reaching real services on the bus: SQS (and its task token round trip), DynamoDB, an `aws-sdk:` call, a child execution
//! with `.sync:2`, an activity worker, a service that does not run here, and logging to CloudWatch Logs.

use serde_json::{Value, json};

use super::{World, output};

async fn queue(w: &World, name: &str) -> String {
    let q = w
        .peer("sqs", "CreateQueue", json!({"QueueName": name}))
        .await;
    q["QueueUrl"].as_str().unwrap().to_owned()
}

async fn receive(w: &World, url: &str) -> Value {
    for _ in 0..100 {
        let got = w
            .peer(
                "sqs",
                "ReceiveMessage",
                json!({"QueueUrl": url, "MaxNumberOfMessages": 1}),
            )
            .await;
        if let Some(m) = got["Messages"].as_array().and_then(|a| a.first()) {
            return serde_json::from_str(m["Body"].as_str().unwrap()).unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("no message on {url}");
}

#[tokio::test]
async fn sqs_send_and_a_task_token_round_trip() {
    let w = World::new();
    let url = queue(&w, "jobs").await;
    let m = w
        .machine(
            "q",
            json!({"StartAt": "Send", "States": {
                "Send": {"Type": "Task", "Resource": "arn:aws:states:::sqs:sendMessage",
                    "Parameters": {"QueueUrl": url, "MessageBody": {"hello.$": "$.who"}}, "ResultPath": "$.sent", "Next": "Wait"},
                "Wait": {"Type": "Task", "Resource": "arn:aws:states:::sqs:sendMessage.waitForTaskToken",
                    "Parameters": {"QueueUrl": url, "MessageBody": {"token.$": "$$.Task.Token"}}, "End": true}}}),
        )
        .await;
    let e = w.start(&m, json!({"who": "queue"})).await;
    assert_eq!(receive(&w, &url).await, json!({"hello": "queue"}));
    let token = receive(&w, &url).await["token"]
        .as_str()
        .unwrap()
        .to_owned();
    w.ok("SendTaskHeartbeat", json!({"taskToken": token})).await;
    w.ok(
        "SendTaskSuccess",
        json!({"taskToken": token, "output": "{\"answered\": true}"}),
    )
    .await;
    let d = w.finished(&e).await;
    assert_eq!(output(&d), json!({"answered": true}));
    let spent = w
        .call(
            "SendTaskSuccess",
            json!({"taskToken": token, "output": "{}"}),
        )
        .await
        .unwrap_err();
    assert_eq!(spent.code, "TaskDoesNotExist");
    let h = w.history(&e).await;
    for kind in [
        "TaskScheduled",
        "TaskStarted",
        "TaskSubmitted",
        "TaskSucceeded",
    ] {
        assert!(h.contains(&kind.to_owned()), "{kind} missing from {h:?}");
    }
    w.stop().await;
}

#[tokio::test]
async fn dynamodb_put_and_get_with_a_caught_condition() {
    let w = World::new();
    w.peer(
        "dynamodb",
        "CreateTable",
        json!({"TableName": "tbl", "AttributeDefinitions": [{"AttributeName": "id", "AttributeType": "S"}],
            "KeySchema": [{"AttributeName": "id", "KeyType": "HASH"}], "BillingMode": "PAY_PER_REQUEST"}),
    )
    .await;
    let m = w
        .machine(
            "d",
            json!({"StartAt": "Put", "States": {
                "Put": {"Type": "Task", "Resource": "arn:aws:states:::dynamodb:putItem",
                    "Parameters": {"TableName": "tbl", "Item": {"id": {"S.$": "$.id"}}}, "ResultPath": null, "Next": "Again"},
                "Again": {"Type": "Task", "Resource": "arn:aws:states:::dynamodb:putItem",
                    "Parameters": {"TableName": "tbl", "Item": {"id": {"S.$": "$.id"}}, "ConditionExpression": "attribute_not_exists(id)"},
                    "Catch": [{"ErrorEquals": ["DynamoDB.ConditionalCheckFailedException"], "ResultPath": "$.caught", "Next": "Get"}],
                    "End": true},
                "Get": {"Type": "Task", "Resource": "arn:aws:states:::aws-sdk:dynamodb:getItem",
                    "Parameters": {"TableName": "tbl", "Key": {"id": {"S.$": "$.id"}}}, "ResultSelector": {"item.$": "$.Item.id.S"},
                    "ResultPath": "$.got", "End": true}}}),
        )
        .await;
    let d = w.run(&m, json!({"id": "a1"})).await;
    let out = output(&d);
    assert_eq!(
        out["caught"]["Error"],
        "DynamoDB.ConditionalCheckFailedException"
    );
    assert_eq!(out["got"], json!({"item": "a1"}));
    w.stop().await;
}

#[tokio::test]
async fn a_child_execution_with_sync_2_and_an_activity_worker() {
    let w = World::new();
    let act = w.ok("CreateActivity", json!({"name": "worker"})).await["activityArn"]
        .as_str()
        .unwrap()
        .to_owned();
    let child = w
        .machine("child", json!({"StartAt": "Work", "States": {"Work": {"Type": "Task", "Resource": act, "End": true}}}))
        .await;
    let parent = w
        .machine(
            "parent",
            json!({"StartAt": "Call", "States": {"Call": {"Type": "Task", "Resource": "arn:aws:states:::states:startExecution.sync:2",
                "Parameters": {"StateMachineArn": child, "Input": {"n.$": "$.n"}}, "OutputPath": "$.Output", "End": true}}}),
        )
        .await;
    let e = w.start(&parent, json!({"n": 4})).await;
    let task = w
        .ok(
            "GetActivityTask",
            json!({"activityArn": act, "workerName": "w1"}),
        )
        .await;
    assert_eq!(
        serde_json::from_str::<Value>(task["input"].as_str().unwrap()).unwrap(),
        json!({"n": 4})
    );
    let token = task["taskToken"].as_str().unwrap();
    w.ok(
        "SendTaskSuccess",
        json!({"taskToken": token, "output": "{\"doubled\": 8}"}),
    )
    .await;
    let d = w.finished(&e).await;
    assert_eq!(output(&d), json!({"doubled": 8}), "{d}");
    w.stop().await;
}

#[tokio::test]
async fn a_service_that_does_not_run_here_fails_the_state_catchably() {
    let w = World::new();
    let m = w
        .machine(
            "ecs",
            json!({"StartAt": "Run", "States": {
                "Run": {"Type": "Task", "Resource": "arn:aws:states:::ecs:runTask.sync", "Parameters": {"Cluster": "c"},
                    "Catch": [{"ErrorEquals": ["States.TaskFailed"], "Next": "Handled"}], "End": true},
                "Handled": {"Type": "Pass", "End": true}}}),
        )
        .await;
    let d = w.run(&m, json!({})).await;
    assert_eq!(d["status"], "SUCCEEDED");
    assert_eq!(output(&d)["Error"], "States.TaskFailed");
    w.stop().await;
}

#[tokio::test]
async fn logging_writes_the_events_to_the_group() {
    let w = World::new();
    w.peer(
        "logs",
        "CreateLogGroup",
        json!({"logGroupName": "/sfn/test"}),
    )
    .await;
    let group = "arn:aws:logs:us-east-1:000000000000:log-group:/sfn/test:*";
    let def = json!({"StartAt": "A", "States": {"A": {"Type": "Pass", "End": true}}});
    let made = w
        .ok(
            "CreateStateMachine",
            json!({"name": "logged", "definition": def.to_string(), "roleArn": super::ROLE,
                "loggingConfiguration": {"level": "ALL", "includeExecutionData": true,
                    "destinations": [{"cloudWatchLogsLogGroup": {"logGroupArn": group}}]}}),
        )
        .await;
    w.run(made["stateMachineArn"].as_str().unwrap(), json!({}))
        .await;
    for _ in 0..100 {
        let got = w
            .peer(
                "logs",
                "FilterLogEvents",
                json!({"logGroupName": "/sfn/test"}),
            )
            .await;
        let events = got["events"].as_array().cloned().unwrap_or_default();
        if events.len() >= 4 {
            let first: Value =
                serde_json::from_str(events[0]["message"].as_str().unwrap()).unwrap();
            assert_eq!(first["type"], "ExecutionStarted");
            w.stop().await;
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("the log records never arrived");
}
