//! A resource's tags, read and changed through each service's own tag calls: set, replace, remove, as the console's caller,
//! against real services on a real bus.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]

#[path = "resources/code.rs"]
mod code;
mod support;
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use serde_json::{Value, json};
use support::{keys, ts_keys};
use world::{World, encode, world};

const BASE: &str = "/api/v1/resources";

fn tags_of(view: &Value) -> Vec<(String, String)> {
    view["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                t["key"].as_str().unwrap().to_owned(),
                t["value"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn pair(k: &str, v: &str) -> (String, String) {
    (k.to_owned(), v.to_owned())
}

/// A stack is updated again only once its last update is complete (CloudFormation's own rule): wait for it.
async fn settle(w: &World, path: &str) {
    if !path.starts_with("cloudformation/") {
        return;
    }
    for _ in 0..200 {
        let (_, d) = w.get(&format!("{BASE}/{path}")).await;
        let status = d["fields"][1]["value"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        if status.ends_with("_COMPLETE") {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

/// A resource at `path` (`sqs/queues/orders`): none, two, one changed and one removed, none again.
async fn round_trip(w: &World, path: &str) {
    let url = format!("{BASE}/{path}/tags");
    let (status, view) = w.get(&url).await;
    assert_eq!(status, 200, "{path}: {view}");
    assert_eq!(keys(&view), ts_keys("TagsView"));
    assert_eq!(view["editable"], true, "{path}");
    assert!(tags_of(&view).is_empty(), "{path}: {view}");
    let set = json!({"set": [{"key": "env", "value": "dev"}, {"key": "team", "value": "orders"}]});
    settle(w, path).await;
    let (status, view) = w.send("PUT", &url, &set).await;
    assert_eq!(status, 200, "{path}: {view}");
    assert_eq!(
        tags_of(&view),
        [pair("env", "dev"), pair("team", "orders")],
        "{path}"
    );
    let change = json!({"set": [{"key": "env", "value": "prod"}], "remove": ["team"]});
    settle(w, path).await;
    let (status, view) = w.send("PUT", &url, &change).await;
    assert_eq!(status, 200, "{path}: {view}");
    assert_eq!(tags_of(&view), [pair("env", "prod")], "{path}");
    let (_, again) = w.get(&url).await;
    assert_eq!(tags_of(&again), [pair("env", "prod")], "{path}");
    settle(w, path).await;
    let (status, view) = w.send("PUT", &url, &json!({"remove": ["env"]})).await;
    assert_eq!(status, 200, "{path}: {view}");
    assert!(tags_of(&view).is_empty(), "{path}: {view}");
    // The detail says there are tags to read.
    let (_, detail) = w.get(&format!("{BASE}/{path}")).await;
    assert_eq!(detail["tags"], true, "{path}");
}

#[tokio::test]
async fn messaging_and_storage_resources_are_tagged_through_their_own_calls() {
    let w = world("tags-a", Mode::Enforce, &[]);
    w.app("sqs", "CreateQueue", &json!({"QueueName": "orders"}))
        .await;
    round_trip(&w, "sqs/queues/orders").await;
    w.app("sns", "CreateTopic", &json!({"Name": "events"}))
        .await;
    round_trip(&w, "sns/topics/events").await;
    // S3 writes tags whole: a change reads the set, changes it and puts it back; none is a delete.
    w.app("s3", "CreateBucket", &json!({"Bucket": "images"}))
        .await;
    round_trip(&w, "s3/buckets/images").await;
    w.app(
        "dynamodb",
        "CreateTable",
        &json!({"TableName": "orders", "BillingMode": "PAY_PER_REQUEST",
            "AttributeDefinitions": [{"AttributeName": "id", "AttributeType": "S"}],
            "KeySchema": [{"AttributeName": "id", "KeyType": "HASH"}]}),
    )
    .await;
    round_trip(&w, "dynamodb/tables/orders").await;
    w.app(
        "kinesis",
        "CreateStream",
        &json!({"StreamName": "clicks", "StreamModeDetails": {"StreamMode": "ON_DEMAND"}}),
    )
    .await;
    round_trip(&w, "kinesis/streams/clicks").await;
    // The tags were the console's calls.
    let live = w.live().join("\n");
    for call in [
        "TagQueue",
        "UntagQueue",
        "ListQueueTags",
        "PutBucketTagging",
        "DeleteBucketTagging",
    ] {
        assert!(
            live.contains("console") && live.contains(call),
            "{call}: {live}"
        );
    }
}

#[tokio::test]
async fn config_identity_and_flow_resources_are_tagged_through_their_own_calls() {
    let w = world("tags-b", Mode::Enforce, &[]);
    let key = w.app("kms", "CreateKey", &json!({})).await;
    let key_id = key["KeyMetadata"]["KeyId"].as_str().unwrap().to_owned();
    round_trip(&w, &format!("kms/keys/{key_id}")).await;
    w.app(
        "secretsmanager",
        "CreateSecret",
        &json!({"Name": "db", "SecretString": "x"}),
    )
    .await;
    round_trip(&w, "secretsmanager/secrets/db").await;
    w.app(
        "ssm",
        "PutParameter",
        &json!({"Name": "/app/size", "Type": "String", "Value": "1"}),
    )
    .await;
    round_trip(&w, &format!("ssm/parameters/{}", encode("/app/size"))).await;
    w.app(
        "logs",
        "CreateLogGroup",
        &json!({"logGroupName": "/app/web"}),
    )
    .await;
    round_trip(&w, &format!("logs/groups/{}", encode("/app/web"))).await;
    let trust = json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow",
        "Principal": {"Service": "lambda.amazonaws.com"}, "Action": "sts:AssumeRole"}]});
    w.app(
        "iam",
        "CreateRole",
        &json!({"RoleName": "runner", "AssumeRolePolicyDocument": trust.to_string()}),
    )
    .await;
    round_trip(&w, "iam/roles/runner").await;
    w.app("iam", "CreateUser", &json!({"UserName": "ci"})).await;
    round_trip(&w, "iam/users/ci").await;
    let doc = json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Action": "s3:GetObject", "Resource": "*"}]});
    w.app(
        "iam",
        "CreatePolicy",
        &json!({"PolicyName": "readonly", "PolicyDocument": doc.to_string()}),
    )
    .await;
    round_trip(
        &w,
        &format!(
            "iam/policies/{}",
            encode("arn:aws:iam::000000000000:policy/readonly")
        ),
    )
    .await;
}

#[tokio::test]
async fn events_workflows_alarms_and_stacks_are_tagged_through_their_own_calls() {
    let w = world("tags-c", Mode::Enforce, &[]);
    w.app("events", "CreateEventBus", &json!({"Name": "larder"}))
        .await;
    round_trip(&w, "eventbridge/buses/larder").await;
    w.app(
        "events",
        "PutRule",
        &json!({"Name": "on-order", "EventBusName": "larder", "EventPattern": "{\"source\":[\"x\"]}"}),
    )
    .await;
    round_trip(
        &w,
        &format!("eventbridge/rules/{}", encode("larder/on-order")),
    )
    .await;
    // The default bus's rules are named by themselves.
    w.app(
        "events",
        "PutRule",
        &json!({"Name": "nightly", "ScheduleExpression": "rate(1 day)"}),
    )
    .await;
    round_trip(&w, "eventbridge/rules/nightly").await;
    let definition =
        json!({"StartAt": "A", "States": {"A": {"Type": "Pass", "End": true}}}).to_string();
    let trust = json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow",
        "Principal": {"Service": "states.amazonaws.com"}, "Action": "sts:AssumeRole"}]});
    w.app(
        "iam",
        "CreateRole",
        &json!({"RoleName": "sfn", "AssumeRolePolicyDocument": trust.to_string()}),
    )
    .await;
    w.app(
        "states",
        "CreateStateMachine",
        &json!({"name": "checkout", "definition": definition,
            "roleArn": "arn:aws:iam::000000000000:role/sfn"}),
    )
    .await;
    round_trip(&w, "stepfunctions/machines/checkout").await;
    w.app(
        "monitoring",
        "PutMetricAlarm",
        &json!({"AlarmName": "queue-deep", "MetricName": "ApproximateNumberOfMessagesVisible",
            "Namespace": "AWS/SQS", "Statistic": "Maximum", "Period": 60, "EvaluationPeriods": 1,
            "Threshold": 100.0, "ComparisonOperator": "GreaterThanThreshold"}),
    )
    .await;
    round_trip(&w, "cloudwatch/alarms/queue-deep").await;
    // A stack with a parameter that has no default: its tags are set without asking for the value again.
    let template = json!({"Parameters": {"Env": {"Type": "String"}},
        "Resources": {"Inbox": {"Type": "AWS::SQS::Queue", "Properties": {"QueueName": {"Fn::Sub": "stack-${Env}"}}}}});
    w.app(
        "cloudformation",
        "CreateStack",
        &json!({"StackName": "larder", "TemplateBody": template.to_string(),
            "Parameters": [{"ParameterKey": "Env", "ParameterValue": "dev"}]}),
    )
    .await;
    for _ in 0..100 {
        let (_, d) = w.get(&format!("{BASE}/cloudformation/stacks/larder")).await;
        if d["fields"][1]["value"] == "CREATE_COMPLETE" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    round_trip(&w, "cloudformation/stacks/larder").await;
}

#[tokio::test]
async fn tags_are_refused_in_words_when_they_cannot_be_read_or_changed() {
    let w = world("tags-refusals", Mode::Soft, &[]);
    w.app("sqs", "CreateQueue", &json!({"QueueName": "orders"}))
        .await;
    // A kind with no tags entry.
    let (status, body) = w.get(&format!("{BASE}/apigateway/restapis/x/tags")).await;
    assert_eq!(
        (status, body["code"].as_str()),
        (404, Some("NO_TAGS")),
        "{body}"
    );
    // A resource that is not there.
    let (status, body) = w.get(&format!("{BASE}/sqs/queues/nothing/tags")).await;
    assert_eq!(status, 400, "{body}");
    // A tag with no key, and a body that is not a change.
    let url = format!("{BASE}/sqs/queues/orders/tags");
    let (status, body) = w
        .send("PUT", &url, &json!({"set": [{"key": " ", "value": "x"}]}))
        .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (400, Some("BAD_REQUEST")),
        "{body}"
    );
    let (status, body) = w.send("PUT", &url, &json!([1])).await;
    assert_eq!(
        (status, body["code"].as_str()),
        (400, Some("BAD_REQUEST")),
        "{body}"
    );
    // The service's own refusal passes through (a reserved prefix).
    let (status, body) = w
        .send(
            "PUT",
            &url,
            &json!({"set": [{"key": "aws:x", "value": "1"}]}),
        )
        .await;
    assert!(status >= 400, "{status}: {body}");
}

/// A function's tags (it needs code: a test without the `zip` program does nothing).
#[tokio::test]
async fn a_function_is_tagged_through_lambdas_own_calls() {
    let w = world("tags-lambda", Mode::Enforce, &[]);
    if code::function(&w, "echo").await {
        round_trip(&w, "lambda/functions/echo").await;
    }
}
