//! What exists for local testing (Wait scaling, Step Functions Local's mock configuration), and a distributed Map that reads its
//! items from S3 and writes its results there.

use serde_json::{Value, json};

use super::{World, output};

#[tokio::test]
async fn a_scaled_wait_and_a_mocked_task() {
    let config = crate::Config {
        wait_scale: 0.0005,
        mocks: Some(json!({
            "StateMachines": {"mocked": {"TestCases": {"happy": {"Call": "answer"}, "sad": {"Call": "boom"}}}},
            "MockedResponses": {
                "answer": {"0": {"Return": {"Payload": 7}}},
                "boom": {"0-2": {"Throw": {"Error": "Lambda.ServiceException", "Cause": "mocked"}}}
            }
        })),
    };
    let w = World::configured(aws_testkit::TempDir::new("sfn-local"), config);
    let m = w
        .machine(
            "mocked",
            json!({"StartAt": "Hour", "States": {
                "Hour": {"Type": "Wait", "Seconds": 3600, "Next": "Call"},
                "Call": {"Type": "Task", "Resource": "arn:aws:states:::lambda:invoke",
                    "Parameters": {"FunctionName": "not-here"}, "OutputPath": "$.Payload", "End": true}}}),
        )
        .await;
    let e = w.start(&format!("{m}#happy"), json!({})).await;
    let d = w.finished(&e).await;
    assert_eq!(output(&d), json!(7));
    let sad = w.start(&format!("{m}#sad"), json!({})).await;
    let d = w.finished(&sad).await;
    assert_eq!(
        (d["status"].as_str(), d["error"].as_str()),
        (Some("FAILED"), Some("Lambda.ServiceException"))
    );
    w.stop().await;
}

#[tokio::test]
async fn a_distributed_map_reads_from_s3_and_writes_its_results_there() {
    let w = World::new();
    let sdk = |action: &str| format!("arn:aws:states:::aws-sdk:s3:{action}");
    let m = w
        .machine(
            "s3map",
            json!({"StartAt": "Bucket", "States": {
                "Bucket": {"Type": "Task", "Resource": sdk("createBucket"), "Parameters": {"Bucket": "items-b"},
                    "ResultPath": null, "Next": "Put"},
                "Put": {"Type": "Task", "Resource": sdk("putObject"),
                    "Parameters": {"Bucket": "items-b", "Key": "in.json", "Body": "[1, 2, 3]"}, "ResultPath": null, "Next": "Each"},
                "Each": {"Type": "Map",
                    "ItemReader": {"Resource": "arn:aws:states:::s3:getObject", "ReaderConfig": {"InputType": "JSON"},
                        "Parameters": {"Bucket": "items-b", "Key": "in.json"}},
                    "ItemProcessor": {"ProcessorConfig": {"Mode": "DISTRIBUTED", "ExecutionType": "STANDARD"},
                        "StartAt": "Tag", "States": {"Tag": {"Type": "Pass", "Parameters": {"v.$": "$"}, "End": true}}},
                    "ResultWriter": {"Resource": "arn:aws:states:::s3:putObject", "Parameters": {"Bucket": "items-b", "Prefix": "out"}},
                    "Next": "Manifest"},
                "Manifest": {"Type": "Task", "Resource": sdk("getObject"),
                    "Parameters": {"Bucket.$": "$.ResultWriterDetails.Bucket", "Key.$": "$.ResultWriterDetails.Key"},
                    "OutputPath": "$.Body", "End": true}}}),
        )
        .await;
    let d = w.run(&m, json!({})).await;
    assert_eq!(d["status"], "SUCCEEDED", "{d}");
    let manifest: Value = serde_json::from_str(output(&d).as_str().unwrap()).unwrap();
    let files = manifest["ResultFiles"]["SUCCEEDED"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert!(files[0]["Key"].as_str().unwrap().starts_with("out/"));
    w.stop().await;
}
