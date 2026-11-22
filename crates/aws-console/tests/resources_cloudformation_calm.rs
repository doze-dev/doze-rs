//! CloudFormation's refusals that only say there is nothing (`Kind::calm`): the console reads them as empty answers, so a stack page makes no
//! request the browser logs as failed; every other refusal is passed on as it is.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]

mod support;
#[path = "resources/world.rs"]
mod world;

use std::time::Duration;

use aws_peers::Mode;
use serde_json::{Value, json};
use world::world;

async fn op(w: &world::World, operation: &str, input: &Value) -> (u16, Value) {
    w.send(
        "POST",
        &format!("/api/v1/resources/cloudformation/stacks/op/{operation}"),
        input,
    )
    .await
}

#[tokio::test]
async fn a_refusal_that_says_there_is_nothing_is_an_empty_answer_and_no_other_is() {
    let w = world("cfn-calm", Mode::Soft, &[]);
    let body = json!({"Resources": {"Inbox": {"Type": "AWS::SQS::Queue", "Properties": {"QueueName": "calm-inbox"}}},
        "Outputs": {"Arn": {"Value": {"Fn::GetAtt": ["Inbox", "Arn"]}, "Export": {"Name": "calm-arn"}}}})
    .to_string();
    let (status, made) = op(
        &w,
        "CreateStack",
        &json!({"StackName": "calm", "TemplateBody": body}),
    )
    .await;
    assert_eq!(status, 200, "{made}");
    for _ in 0..100 {
        let (_, d) = op(&w, "DescribeStacks", &json!({"StackName": "calm"})).await;
        if d["Stacks"][0]["StackStatus"] == "CREATE_COMPLETE" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // An export nobody imports: an empty list (a page of a stack asks for every export).
    let (status, nobody) = op(&w, "ListImports", &json!({"ExportName": "calm-arn"})).await;
    assert_eq!((status, &nobody), (200, &json!({"Imports": []})));
    // An export that does not exist is a refusal still: only "nobody imports it" is calm.
    let (status, none) = op(&w, "ListImports", &json!({"ExportName": "exp-none"})).await;
    assert_eq!(
        (status, none["code"].as_str()),
        (400, Some("ValidationError")),
        "{none}"
    );
    // A stack that is gone, read by its name.
    let (status, gone) = op(&w, "DescribeStacks", &json!({"StackName": "nope"})).await;
    assert_eq!((status, &gone), (200, &json!({"Stacks": []})));
    let (status, gone) = op(&w, "ListStackResources", &json!({"StackName": "nope"})).await;
    assert_eq!(
        (status, &gone),
        (200, &json!({"StackResourceSummaries": []}))
    );
    // A stack that is there is read as it is.
    let (_, there) = op(&w, "DescribeStacks", &json!({"StackName": "calm"})).await;
    assert_eq!(there["Stacks"][0]["StackName"], "calm", "{there}");
    // Any other refusal passes as it is: a change set that is not there.
    let (status, other) = op(
        &w,
        "DescribeChangeSet",
        &json!({"StackName": "calm", "ChangeSetName": "nope"}),
    )
    .await;
    assert_eq!(
        (status, other["code"].as_str()),
        (404, Some("ChangeSetNotFound")),
        "{other}"
    );
}
