//! The Resources API against real services on a real bus, the services of flow and observation: CloudWatch Logs, Kinesis,
//! CloudWatch, EventBridge, Scheduler and Step Functions, each made, listed, opened (with its related lists) and deleted as the
//! caller `console`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use serde_json::json;
use world::{encode, field, firsts, sub, world};

#[tokio::test]
async fn a_log_group_shows_its_streams_newest_first_and_a_missing_one_is_refused() {
    let w = world("logs", Mode::Soft, &[]);
    let form = json!({"logGroupName": "/larder/api", "logGroupClass": "STANDARD"});
    w.round_trip("/api/v1/resources/logs/groups", &form, "/larder/api")
        .await;
    // A group whose name the first begins with is not the first's.
    w.app(
        "logs",
        "CreateLogGroup",
        &json!({"logGroupName": "/larder/api-v2"}),
    )
    .await;
    for s in ["older", "newer"] {
        w.app(
            "logs",
            "CreateLogStream",
            &json!({"logGroupName": "/larder/api", "logStreamName": s}),
        )
        .await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        w.app(
            "logs",
            "PutLogEvents",
            &json!({"logGroupName": "/larder/api", "logStreamName": s, "logEvents": [{"timestamp": now, "message": s}]}),
        )
        .await;
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let one = format!("/api/v1/resources/logs/groups/{}", encode("/larder/api"));
    let (_, detail) = w.get(&one).await;
    // Its streams are a page of their own: the page reads them through the group.
    let (status, streams) = w
        .send(
            "POST",
            &format!("{one}/op/DescribeLogStreams"),
            &json!({"orderBy": "LastEventTime", "descending": true}),
        )
        .await;
    assert_eq!(status, 200, "{streams}");
    let names: Vec<&str> = streams["logStreams"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["logStreamName"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["newer", "older"]);
    assert_eq!(field(&detail, "Retention (days)")["value"], "Never expire");
    assert!(
        field(&detail, "ARN")["value"]
            .as_str()
            .unwrap()
            .ends_with(":log-group:/larder/api:*")
            || field(&detail, "ARN")["value"]
                .as_str()
                .unwrap()
                .ends_with(":log-group:/larder/api"),
        "{detail}"
    );
    let (status, missing) = w
        .get(&format!(
            "/api/v1/resources/logs/groups/{}",
            encode("/larder")
        ))
        .await;
    assert_eq!((status, missing["code"].as_str()), (404, Some("NOT_FOUND")));
    w.delete("/api/v1/resources/logs/groups", "/larder/api")
        .await;
}

#[tokio::test]
async fn a_stream_shows_its_shards_and_an_alarm_is_read_and_deleted() {
    let w = world("kinesis", Mode::Soft, &[]);
    let form = json!({"StreamName": "clicks", "StreamMode": "PROVISIONED", "ShardCount": "2"});
    w.round_trip("/api/v1/resources/kinesis/streams", &form, "clicks")
        .await;
    let (_, detail) = w.get("/api/v1/resources/kinesis/streams/clicks").await;
    assert_eq!(sub(&detail, "Shards")["rows"].as_array().unwrap().len(), 2);
    w.delete("/api/v1/resources/kinesis/streams", "clicks")
        .await;

    w.app(
        "monitoring",
        "PutMetricAlarm",
        &json!({"AlarmName": "queue-deep", "MetricName": "ApproximateNumberOfMessagesVisible", "Namespace": "AWS/SQS",
            "Statistic": "Maximum", "Period": 60, "EvaluationPeriods": 1, "Threshold": 100.0,
            "ComparisonOperator": "GreaterThanThreshold", "Dimensions": [{"Name": "QueueName", "Value": "orders"}]}),
    )
    .await;
    let (_, detail) = w
        .get("/api/v1/resources/cloudwatch/alarms/queue-deep")
        .await;
    assert_eq!(field(&detail, "Dimensions")["value"], "QueueName=orders");
    assert_eq!(field(&detail, "Threshold")["value"], "100.0");
    let (status, _) = w.get("/api/v1/resources/cloudwatch/alarms/nope").await;
    assert_eq!(status, 404);
    w.delete("/api/v1/resources/cloudwatch/alarms", "queue-deep")
        .await;
    w.app(
        "monitoring",
        "PutMetricData",
        &json!({"Namespace": "Larder", "MetricData": [{"MetricName": "Orders", "Value": 1.0, "Dimensions": [{"Name": "env", "Value": "dev"}]}]}),
    )
    .await;
    let (_, metrics) = w.get("/api/v1/resources/cloudwatch/metrics").await;
    assert_eq!(metrics["openable"], false);
    let row = metrics["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["label"] == "Orders")
        .unwrap_or_else(|| panic!("{metrics}"));
    assert_eq!(row["cells"], json!(["Larder", "env=dev"]));
}

#[tokio::test]
async fn a_rule_on_a_custom_bus_is_named_with_its_bus_and_lists_its_targets() {
    let w = world("events", Mode::Soft, &[]);
    w.round_trip(
        "/api/v1/resources/eventbridge/buses",
        &json!({"Name": "orders"}),
        "orders",
    )
    .await;
    let form = json!({"Name": "on-order", "EventBusName": "orders", "EventPattern": r#"{"source":["larder"]}"#, "State": "ENABLED"});
    let (status, made) = w
        .send("POST", "/api/v1/resources/eventbridge/rules", &form)
        .await;
    assert_eq!(
        (status, made["name"].as_str()),
        (201, Some("orders/on-order")),
        "{made}"
    );
    w.app(
        "events",
        "PutTargets",
        &json!({"Rule": "on-order", "EventBusName": "orders", "Targets": [{"Id": "q", "Arn": "arn:aws:sqs:us-east-1:000000000000:inbox"}]}),
    )
    .await;
    let one = format!(
        "/api/v1/resources/eventbridge/rules/{}",
        encode("orders/on-order")
    );
    let (status, rule) = w.get(&one).await;
    assert_eq!(status, 200, "{rule}");
    assert_eq!(firsts(sub(&rule, "Targets")), ["inbox"]);
    assert_eq!(field(&rule, "Event bus")["link"]["name"], "orders");
    let (_, bus) = w.get("/api/v1/resources/eventbridge/buses/orders").await;
    assert_eq!(
        sub(&bus, "Rules")["rows"][0]["link"]["name"],
        "orders/on-order"
    );
    // A rule with targets is refused by EventBridge.
    let (status, _) = w.send("DELETE", &one, &json!({})).await;
    assert_ne!(status, 200);
    // A rule on the default bus is its bare name.
    let form =
        json!({"Name": "tick", "ScheduleExpression": "rate(5 minutes)", "State": "DISABLED"});
    w.round_trip("/api/v1/resources/eventbridge/rules", &form, "tick")
        .await;
    w.delete("/api/v1/resources/eventbridge/rules", "tick")
        .await;
}

#[tokio::test]
async fn a_schedule_is_named_with_its_group() {
    let w = world("scheduler", Mode::Soft, &[]);
    let form = json!({"Name": "nightly", "ScheduleExpression": "rate(1 day)", "TargetArn": "arn:aws:sqs:us-east-1:000000000000:inbox",
        "RoleArn": "arn:aws:iam::000000000000:role/scheduler", "Input": "hello"});
    let name = w
        .round_trip("/api/v1/resources/scheduler/schedules", &form, "")
        .await;
    assert_eq!(name, "default/nightly");
    let (_, detail) = w
        .get(&format!(
            "/api/v1/resources/scheduler/schedules/{}",
            encode(&name)
        ))
        .await;
    assert_eq!(field(&detail, "Expression")["value"], "rate(1 day)");
    let (_, group) = w.get("/api/v1/resources/scheduler/groups/default").await;
    assert_eq!(
        sub(&group, "Schedules")["rows"][0]["link"]["name"],
        name.as_str()
    );
    w.delete("/api/v1/resources/scheduler/schedules", &name)
        .await;
}

#[tokio::test]
async fn a_state_machine_lists_its_executions_newest_first() {
    let w = world("states", Mode::Soft, &[]);
    let definition = r#"{"StartAt":"Done","States":{"Done":{"Type":"Pass","End":true}}}"#;
    let made = w
        .app(
            "states",
            "CreateStateMachine",
            &json!({"name": "checkout", "definition": definition, "roleArn": "arn:aws:iam::000000000000:role/sfn"}),
        )
        .await;
    let arn = made["stateMachineArn"].as_str().unwrap();
    for n in ["first", "second", "third"] {
        w.app(
            "states",
            "StartExecution",
            &json!({"stateMachineArn": arn, "name": n}),
        )
        .await;
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    }
    let (_, list) = w.get("/api/v1/resources/stepfunctions/machines").await;
    assert_eq!(list["rows"][0]["name"], "checkout");
    assert!(list["create"].is_null());
    assert!(
        list["create_elsewhere"]
            .as_str()
            .unwrap()
            .contains("create-state-machine")
    );
    let (status, detail) = w
        .get("/api/v1/resources/stepfunctions/machines/checkout")
        .await;
    assert_eq!(status, 200, "{detail}");
    assert_eq!(field(&detail, "Role")["link"]["name"], "sfn");
    // Its executions are a page of their own: the page lists them through the machine.
    let (status, runs) = w
        .send(
            "POST",
            "/api/v1/resources/stepfunctions/machines/checkout/op/ListExecutions",
            &json!({}),
        )
        .await;
    assert_eq!(status, 200, "{runs}");
    let names: Vec<&str> = runs["executions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["third", "second", "first"]);
    w.delete("/api/v1/resources/stepfunctions/machines", "checkout")
        .await;
}
