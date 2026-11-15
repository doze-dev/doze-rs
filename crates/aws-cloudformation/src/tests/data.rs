//! `doze-aws data` on CloudFormation: a stack name's size is its stacks, events and change sets; deleted stacks are noted and
//! emptied, the live stack stays untouched, and a reset removes every stack record.

use aws_wire::storage::{Prune, Storage, StorageError};
use serde_json::json;

use super::World;

fn template(n: usize) -> serde_json::Value {
    // A parameter per resource, with a fat description: a template worth measuring.
    let mut resources = serde_json::Map::new();
    for i in 0..n {
        resources.insert(
            format!("P{i}"),
            json!({"Type": "AWS::SSM::Parameter", "Properties": {"Name": format!("/data/{i}"), "Type": "String",
                "Value": "v", "Description": "d".repeat(400)}}),
        );
    }
    json!({"Resources": resources})
}

#[tokio::test]
async fn deleted_stacks_are_noted_and_emptied_and_the_live_one_stays() {
    let w = World::new();
    for _ in 0..3 {
        assert_eq!(w.create("app", template(20), &[]).await, "CREATE_COMPLETE");
        assert_eq!(w.delete("app").await, "DELETE_COMPLETE");
    }
    assert_eq!(w.create("app", template(20), &[]).await, "CREATE_COMPLETE");

    let usage = w.service.usage().await.unwrap();
    let app = usage.resources.iter().find(|r| r.name == "app").unwrap();
    assert!(app.items > 4 * 40, "{app:?}");
    assert_eq!(
        app.notes[0].text,
        "in 3 deleted stacks, kept with their templates and events"
    );

    let dry = Prune {
        dry_run: true,
        ..Prune::default()
    };
    let would = w.service.empty("us-east-1", "app", &dry).await.unwrap();
    assert_eq!(would.freed, 0);
    let done = w
        .service
        .empty("us-east-1", "app", &Prune::default())
        .await
        .unwrap();
    assert_eq!(done.items, would.items);
    assert!(done.freed > 0, "the file shrank: {done:?}");

    // Only the live stack is left, with its events and what it made.
    let listed = w.cfn("ListStacks", json!({})).await.unwrap();
    let summaries = listed["StackSummaries"].as_array().unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0]["StackStatus"], "CREATE_COMPLETE");
    assert!(!w.events("app").await.is_empty());
    w.call("ssm", "GetParameter", json!({"Name": "/data/0"}))
        .await
        .unwrap();

    let prefix = Prune {
        prefix: Some("a".into()),
        ..Prune::default()
    };
    assert!(matches!(
        w.service.empty("us-east-1", "app", &prefix).await,
        Err(StorageError::Unsupported(_))
    ));
    assert_eq!(
        w.service
            .empty("us-east-1", "nope", &Prune::default())
            .await,
        Err(StorageError::NotFound)
    );

    let done = w.service.reset(false).await.unwrap();
    assert!(done.items > 0);
    let listed = w.cfn("ListStacks", json!({})).await.unwrap();
    assert!(listed["StackSummaries"].as_array().unwrap().is_empty());
    // The name is free at once. What the stack made is not CloudFormation's to remove on a reset: `/data/*` is still in SSM.
    let t = json!({"Resources": {"Q": {"Type": "AWS::SQS::Queue", "Properties": {"QueueName": "after-reset"}}}});
    assert_eq!(w.create("app", t, &[]).await, "CREATE_COMPLETE");
    w.call("ssm", "GetParameter", json!({"Name": "/data/0"}))
        .await
        .unwrap();
}
