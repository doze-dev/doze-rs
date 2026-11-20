//! Wait conditions and their signals, and CancelUpdateStack.

use std::time::Duration;

use aws_wire::service::{Web, WebRequest};
use serde_json::{Value, json};

use super::World;

fn queue(name: &str) -> Value {
    json!({"Type": "AWS::SQS::Queue", "Properties": {"QueueName": name}})
}

fn waiting(count: u32, timeout: &str) -> Value {
    json!({"Type": "AWS::CloudFormation::WaitCondition",
        "CreationPolicy": {"ResourceSignal": {"Count": count, "Timeout": timeout}}})
}

async fn start(w: &World, name: &str, template: &Value) {
    w.cfn(
        "CreateStack",
        json!({"StackName": name, "TemplateBody": template.to_string()}),
    )
    .await
    .unwrap();
}

/// Waits until the resource is in progress (its inbox is open).
async fn until_in_progress(w: &World, stack: &str, logical: &str) {
    for _ in 0..400 {
        let got = w
            .cfn(
                "DescribeStackResource",
                json!({"StackName": stack, "LogicalResourceId": logical}),
            )
            .await;
        if let Ok(v) = got
            && v["StackResourceDetail"]["ResourceStatus"]
                .as_str()
                .is_some_and(|s| s.ends_with("_IN_PROGRESS"))
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("{logical} never started");
}

async fn signal(
    w: &World,
    stack: &str,
    logical: &str,
    id: &str,
    status: &str,
) -> Result<Value, aws_wire::ApiError> {
    w.cfn(
        "SignalResource",
        json!({"StackName": stack, "LogicalResourceId": logical, "UniqueId": id, "Status": status}),
    )
    .await
}

#[tokio::test]
async fn a_wait_condition_completes_on_its_signals() {
    let w = World::new();
    let t = json!({"Resources": {"Wait": waiting(2, "PT5M")},
        "Outputs": {"Data": {"Value": {"Fn::GetAtt": ["Wait", "Data"]}}}});
    start(&w, "sig", &t).await;
    until_in_progress(&w, "sig", "Wait").await;
    signal(&w, "sig", "Wait", "a", "SUCCESS").await.unwrap();
    // The same unique id counts once.
    signal(&w, "sig", "Wait", "a", "SUCCESS").await.unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    let s = w
        .cfn("DescribeStacks", json!({"StackName": "sig"}))
        .await
        .unwrap();
    assert_eq!(s["Stacks"][0]["StackStatus"], "CREATE_IN_PROGRESS");
    signal(&w, "sig", "Wait", "b", "SUCCESS").await.unwrap();
    assert_eq!(w.settle("sig").await, "CREATE_COMPLETE");
    let trail = w.events("sig").await;
    // As recorded 2026-11-19: `Resource creation Initiated` as the wait starts, then each signal, then the completion.
    let waits: Vec<(&str, &str)> = trail
        .iter()
        .filter(|(l, _, _)| l == "Wait")
        .map(|(_, s, r)| (s.as_str(), r.as_str()))
        .collect();
    assert_eq!(
        waits.get(..3),
        Some(
            &[
                ("CREATE_IN_PROGRESS", ""),
                ("CREATE_IN_PROGRESS", "Resource creation Initiated"),
                (
                    "CREATE_IN_PROGRESS",
                    "Received SUCCESS signal with UniqueId a"
                ),
            ][..]
        ),
        "{waits:?}"
    );
    assert_eq!(
        waits.iter().rev().take(2).collect::<Vec<_>>(),
        [
            &("CREATE_COMPLETE", ""),
            &(
                "CREATE_IN_PROGRESS",
                "Received SUCCESS signal with UniqueId b"
            ),
        ]
    );
    // Recorded 2026-11-19: `SignalResource` carries no data.
    assert_eq!(w.outputs("sig").await["Data"], "{}");
    // Once complete, a signal is accepted and changes nothing (recorded 2026-11-19).
    signal(&w, "sig", "Wait", "c", "SUCCESS").await.unwrap();
    assert_eq!(w.events("sig").await.len(), trail.len());
}

#[tokio::test]
async fn a_failure_signal_or_the_timeout_fails_the_wait() {
    let w = World::new();
    start(
        &w,
        "fail",
        &json!({"Resources": {"Wait": waiting(1, "PT5M")}}),
    )
    .await;
    until_in_progress(&w, "fail", "Wait").await;
    signal(&w, "fail", "Wait", "i-1", "FAILURE").await.unwrap();
    assert_eq!(w.settle("fail").await, "ROLLBACK_COMPLETE");
    assert_eq!(
        w.reason("fail", "Wait", "CREATE_FAILED").await,
        "Received FAILURE signal with UniqueId i-1"
    );

    start(
        &w,
        "late",
        &json!({"Resources": {"Wait": waiting(1, "PT1S")}}),
    )
    .await;
    assert_eq!(w.settle("late").await, "ROLLBACK_COMPLETE");
    assert_eq!(
        w.reason("late", "Wait", "CREATE_FAILED").await,
        "Failed to receive 1 resource signal(s) within the specified duration"
    );
}

#[tokio::test]
async fn signals_put_to_a_handle_complete_its_wait_condition() {
    let w = World::new();
    let t = json!({"Resources": {
        "Handle": {"Type": "AWS::CloudFormation::WaitConditionHandle"},
        "Wait": {"Type": "AWS::CloudFormation::WaitCondition",
            "Properties": {"Handle": {"Ref": "Handle"}, "Timeout": "300", "Count": 1}},
    }, "Outputs": {"Data": {"Value": {"Fn::GetAtt": ["Wait", "Data"]}},
        "Url": {"Value": {"Ref": "Handle"}}}});
    start(&w, "handle", &t).await;
    until_in_progress(&w, "handle", "Wait").await;
    let got = w
        .cfn(
            "DescribeStackResource",
            json!({"StackName": "handle", "LogicalResourceId": "Handle"}),
        )
        .await
        .unwrap();
    let url = got["StackResourceDetail"]["PhysicalResourceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let (path, query) = url
        .strip_prefix("http://localhost:4566")
        .unwrap()
        .split_once('?')
        .unwrap();
    assert!(w.service.owns("localhost", path));
    let put = |body: &str| WebRequest {
        method: "PUT".into(),
        host: "localhost".into(),
        path: path.to_owned(),
        query: query.to_owned(),
        headers: Vec::new(),
        body: bytes::Bytes::from(body.to_owned()),
        endpoint: "http://localhost:4566".into(),
        signature: aws_wire::service::WebSignature::default(),
    };
    let reply = w
        .service
        .serve(put(
            r#"{"Status":"SUCCESS","Reason":"done","UniqueId":"one","Data":"ready"}"#,
        ))
        .await;
    assert_eq!(reply.status, 200);
    assert_eq!(w.settle("handle").await, "CREATE_COMPLETE");
    assert_eq!(w.outputs("handle").await["Data"], r#"{"one":"ready"}"#);
    // The handle goes with the stack; its URL then answers as an expired one.
    assert_eq!(w.delete("handle").await, "DELETE_COMPLETE");
    assert_eq!(w.service.serve(put("{}")).await.status, 403);
}

#[tokio::test]
async fn signal_resource_refusals() {
    let w = World::new();
    let ghost = signal(&w, "ghost", "Wait", "a", "SUCCESS")
        .await
        .unwrap_err();
    assert_eq!(ghost.message, "Stack ghost does not exist");
    assert_eq!(
        w.create(
            "plain",
            json!({"Resources": {"Q": queue("signals-plain")}}),
            &[]
        )
        .await,
        "CREATE_COMPLETE"
    );
    // A resource the stack does not have, or one that does not wait: accepted, nothing happens (recorded 2026-11-19).
    let before = w.events("plain").await.len();
    signal(&w, "plain", "Nope", "a", "SUCCESS").await.unwrap();
    signal(&w, "plain", "Q", "a", "SUCCESS").await.unwrap();
    assert_eq!(w.events("plain").await.len(), before);
}

#[tokio::test]
async fn cancel_update_stack_rolls_the_update_back() {
    let w = World::new();
    let v1 = json!({"Resources": {"Q": queue("cancel-q")}});
    assert_eq!(w.create("cancel", v1, &[]).await, "CREATE_COMPLETE");
    let refused = w
        .cfn("CancelUpdateStack", json!({"StackName": "cancel"}))
        .await
        .unwrap_err();
    assert_eq!(refused.code, "ValidationError");
    assert_eq!(
        refused.message,
        "CancelUpdateStack cannot be called from current stack status"
    );

    let mut wait = waiting(1, "PT1H");
    wait["DependsOn"] = json!("Extra");
    let v2 = json!({"Resources": {"Q": queue("cancel-q"), "Extra": queue("cancel-extra"), "Wait": wait}});
    w.cfn(
        "UpdateStack",
        json!({"StackName": "cancel", "TemplateBody": v2.to_string()}),
    )
    .await
    .unwrap();
    until_in_progress(&w, "cancel", "Wait").await;
    w.cfn("CancelUpdateStack", json!({"StackName": "cancel"}))
        .await
        .unwrap();
    assert_eq!(w.settle("cancel").await, "UPDATE_ROLLBACK_COMPLETE");
    let gone = w
        .call("sqs", "GetQueueUrl", json!({"QueueName": "cancel-extra"}))
        .await;
    assert!(
        gone.is_err(),
        "the update's queue is deleted by the rollback"
    );
    let trail = w.events("cancel").await;
    let stack: Vec<&str> = trail
        .iter()
        .filter(|(l, _, _)| l == "cancel")
        .map(|(_, s, _)| s.as_str())
        .collect();
    assert!(
        stack.ends_with(&[
            "UPDATE_IN_PROGRESS",
            "UPDATE_ROLLBACK_IN_PROGRESS",
            "UPDATE_ROLLBACK_COMPLETE_CLEANUP_IN_PROGRESS",
            "UPDATE_ROLLBACK_COMPLETE"
        ]),
        "{stack:?}"
    );
    assert_eq!(
        w.reason("cancel", "Wait", "CREATE_FAILED").await,
        "Resource creation cancelled"
    );
    // Updated again, it works.
    let v3 = json!({"Resources": {"Q": {"Type": "AWS::SQS::Queue",
        "Properties": {"QueueName": "cancel-q", "VisibilityTimeout": 40}}}});
    w.cfn(
        "UpdateStack",
        json!({"StackName": "cancel", "TemplateBody": v3.to_string()}),
    )
    .await
    .unwrap();
    assert_eq!(w.settle("cancel").await, "UPDATE_COMPLETE");
}
