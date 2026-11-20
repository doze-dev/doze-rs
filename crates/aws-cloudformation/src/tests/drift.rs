//! Drift detection against the real services: resources changed and deleted out of band, and the ones left alone.

use std::time::Duration;

use serde_json::{Value, json};

use super::World;

fn template() -> Value {
    json!({"Resources": {
        "Queue": {"Type": "AWS::SQS::Queue", "Properties": {"QueueName": "drift-q", "VisibilityTimeout": 30,
            "Tags": [{"Key": "team", "Value": "a"}]}},
        "Param": {"Type": "AWS::SSM::Parameter", "Properties": {"Name": "/drift/p", "Type": "String", "Value": "one",
            "Tags": {"team": "a"}}},
        "Topic": {"Type": "AWS::SNS::Topic", "Properties": {"TopicName": "drift-t"}},
        "Table": {"Type": "AWS::DynamoDB::Table", "Properties": {"TableName": "drift-table", "BillingMode": "PAY_PER_REQUEST",
            "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}],
            "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}]}},
        "Role": {"Type": "AWS::IAM::Role", "Properties": {"RoleName": "drift-role",
            "AssumeRolePolicyDocument": {"Version": "2012-10-17", "Statement": [{"Effect": "Allow",
                "Principal": {"Service": "lambda.amazonaws.com"}, "Action": "sts:AssumeRole"}]}}},
        "Secret": {"Type": "AWS::SecretsManager::Secret", "Properties": {"Name": "drift-secret", "Description": "d",
            "SecretString": "s"}},
        "Key": {"Type": "AWS::KMS::Key", "Properties": {"Description": "drift key"}},
        "Bucket": {"Type": "AWS::S3::Bucket", "Properties": {"BucketName": "drift-bucket"}},
        "Handle": {"Type": "AWS::CloudFormation::WaitConditionHandle"},
    }})
}

async fn status(w: &World, id: &str) -> Value {
    for _ in 0..400 {
        let got = w
            .cfn(
                "DescribeStackDriftDetectionStatus",
                json!({"StackDriftDetectionId": id}),
            )
            .await
            .unwrap();
        if got["DetectionStatus"] != "DETECTION_IN_PROGRESS" {
            return got;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("the detection did not finish");
}

async fn drifts(w: &World) -> Vec<Value> {
    let got = w
        .cfn("DescribeStackResourceDrifts", json!({"StackName": "drift"}))
        .await
        .unwrap();
    got["StackResourceDrifts"].as_array().unwrap().clone()
}

fn of<'a>(all: &'a [Value], logical: &str) -> &'a Value {
    all.iter()
        .find(|d| d["LogicalResourceId"] == logical)
        .unwrap()
}

#[tokio::test]
async fn a_stack_left_alone_is_in_sync() {
    let w = World::new();
    assert_eq!(w.create("drift", template(), &[]).await, "CREATE_COMPLETE");
    let before = w
        .cfn("DescribeStacks", json!({"StackName": "drift"}))
        .await
        .unwrap();
    assert_eq!(
        before["Stacks"][0]["DriftInformation"],
        json!({"StackDriftStatus": "NOT_CHECKED"})
    );
    let id = w
        .cfn("DetectStackDrift", json!({"StackName": "drift"}))
        .await
        .unwrap();
    let s = status(&w, id["StackDriftDetectionId"].as_str().unwrap()).await;
    assert_eq!(s["DetectionStatus"], "DETECTION_COMPLETE", "{s}");
    assert_eq!(
        s["StackDriftStatus"],
        "IN_SYNC",
        "{:#}",
        json!(drifts(&w).await)
    );
    assert_eq!(s["DriftedStackResourceCount"], 0);
    let all = drifts(&w).await;
    assert_eq!(
        all.len(),
        8,
        "every type with a reader, and not the handle: {all:#?}"
    );
    assert!(
        all.iter()
            .all(|d| d["StackResourceDriftStatus"] == "IN_SYNC")
    );
    let after = w
        .cfn("DescribeStacks", json!({"StackName": "drift"}))
        .await
        .unwrap();
    assert_eq!(
        after["Stacks"][0]["DriftInformation"]["StackDriftStatus"],
        "IN_SYNC"
    );
    assert!(after["Stacks"][0]["DriftInformation"]["LastCheckTimestamp"].is_string());
    let handle = w
        .cfn(
            "DescribeStackResource",
            json!({"StackName": "drift", "LogicalResourceId": "Handle"}),
        )
        .await
        .unwrap();
    assert_eq!(
        handle["StackResourceDetail"]["DriftInformation"],
        json!({"StackResourceDriftStatus": "NOT_CHECKED"})
    );
    let refused = w
        .cfn(
            "DetectStackResourceDrift",
            json!({"StackName": "drift", "LogicalResourceId": "Handle"}),
        )
        .await
        .unwrap_err();
    assert_eq!(refused.code, "ValidationError");
    // Recorded 2026-11-19.
    assert_eq!(
        refused.message,
        "Drift detection is not supported for ResourceType AWS::CloudFormation::WaitConditionHandle"
    );
}

#[tokio::test]
async fn changes_made_out_of_band_are_drift() {
    let w = World::new();
    assert_eq!(w.create("drift", template(), &[]).await, "CREATE_COMPLETE");
    let url = w
        .call("sqs", "GetQueueUrl", json!({"QueueName": "drift-q"}))
        .await
        .unwrap()["QueueUrl"]
        .clone();
    w.call(
        "sqs",
        "SetQueueAttributes",
        json!({"QueueUrl": url, "Attributes": {"VisibilityTimeout": "45"}}),
    )
    .await
    .unwrap();
    w.call(
        "sqs",
        "TagQueue",
        json!({"QueueUrl": url, "Tags": {"extra": "x"}}),
    )
    .await
    .unwrap();
    w.call(
        "ssm",
        "PutParameter",
        json!({"Name": "/drift/p", "Value": "two", "Type": "String", "Overwrite": true}),
    )
    .await
    .unwrap();
    let topic = format!("arn:aws:sns:us-east-1:{}:drift-t", w.caller.account_id);
    w.call("sns", "DeleteTopic", json!({"TopicArn": topic}))
        .await
        .unwrap();

    let id = w
        .cfn("DetectStackDrift", json!({"StackName": "drift"}))
        .await
        .unwrap();
    let s = status(&w, id["StackDriftDetectionId"].as_str().unwrap()).await;
    assert_eq!(s["StackDriftStatus"], "DRIFTED");
    assert_eq!(s["DriftedStackResourceCount"], 3);
    let all = drifts(&w).await;
    let q = of(&all, "Queue");
    assert_eq!(q["StackResourceDriftStatus"], "MODIFIED");
    let diffs = q["PropertyDifferences"].as_array().unwrap();
    assert!(diffs.contains(&json!({"PropertyPath": "/VisibilityTimeout", "ExpectedValue": "30", "ActualValue": "45",
        "DifferenceType": "NOT_EQUAL"})), "{diffs:#?}");
    assert!(
        diffs.contains(&json!({"PropertyPath": "/Tags/1", "ExpectedValue": "null",
        "ActualValue": "{\"Key\":\"extra\",\"Value\":\"x\"}", "DifferenceType": "ADD"})),
        "{diffs:#?}"
    );
    let expected: Value = serde_json::from_str(q["ExpectedProperties"].as_str().unwrap()).unwrap();
    assert_eq!(expected["VisibilityTimeout"], 30);
    let p = of(&all, "Param");
    assert_eq!(p["PropertyDifferences"][0]["PropertyPath"], "/Value");
    assert_eq!(p["PropertyDifferences"][0]["ActualValue"], "two");
    let t = of(&all, "Topic");
    assert_eq!(t["StackResourceDriftStatus"], "DELETED");
    assert!(t.get("ExpectedProperties").is_none());
    assert_eq!(of(&all, "Table")["StackResourceDriftStatus"], "IN_SYNC");

    let only = w
        .cfn(
            "DescribeStackResourceDrifts",
            json!({"StackName": "drift",
            "StackResourceDriftStatusFilters": ["DELETED"]}),
        )
        .await
        .unwrap();
    assert_eq!(only["StackResourceDrifts"].as_array().unwrap().len(), 1);
    let summaries = w
        .cfn("ListStackResources", json!({"StackName": "drift"}))
        .await
        .unwrap();
    let queue = summaries["StackResourceSummaries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["LogicalResourceId"] == "Queue")
        .unwrap();
    assert_eq!(
        queue["DriftInformation"]["StackResourceDriftStatus"],
        "MODIFIED"
    );

    // Put back by hand, one resource checks in sync at the call.
    w.call(
        "sqs",
        "SetQueueAttributes",
        json!({"QueueUrl": url, "Attributes": {"VisibilityTimeout": "30"}}),
    )
    .await
    .unwrap();
    w.call(
        "sqs",
        "UntagQueue",
        json!({"QueueUrl": url, "TagKeys": ["extra"]}),
    )
    .await
    .unwrap();
    let one = w
        .cfn(
            "DetectStackResourceDrift",
            json!({"StackName": "drift", "LogicalResourceId": "Queue"}),
        )
        .await
        .unwrap();
    assert_eq!(
        one["StackResourceDrift"]["StackResourceDriftStatus"],
        "IN_SYNC"
    );
    // A named resource the stack does not have: the detection still starts (recorded 2026-11-19).
    let absent = w
        .cfn(
            "DetectStackDrift",
            json!({"StackName": "drift", "LogicalResourceIds": ["Ghost"]}),
        )
        .await
        .unwrap();
    assert!(absent["StackDriftDetectionId"].is_string());
    let unknown = w
        .cfn(
            "DescribeStackDriftDetectionStatus",
            json!({"StackDriftDetectionId": "00000000-0000-0000-0000-000000000000"}),
        )
        .await
        .unwrap_err();
    assert_eq!(unknown.code, "ValidationError");
}

#[tokio::test]
async fn drift_results_go_with_a_reset() {
    let w = World::new();
    assert_eq!(w.create("drift", template(), &[]).await, "CREATE_COMPLETE");
    let id = w
        .cfn(
            "DetectStackDrift",
            json!({"StackName": "drift", "LogicalResourceIds": ["Queue"]}),
        )
        .await
        .unwrap();
    let s = status(&w, id["StackDriftDetectionId"].as_str().unwrap()).await;
    assert_eq!(s["StackDriftStatus"], "IN_SYNC");
    assert_eq!(drifts(&w).await.len(), 1, "only the resource asked for");
    let done = aws_wire::storage::Storage::reset(w.service.as_ref(), false)
        .await
        .unwrap();
    assert!(done.bytes > 0);
    let after = w
        .cfn(
            "DescribeStackDriftDetectionStatus",
            json!({"StackDriftDetectionId": id["StackDriftDetectionId"]}),
        )
        .await
        .unwrap_err();
    assert_eq!(after.code, "ValidationError");
}
