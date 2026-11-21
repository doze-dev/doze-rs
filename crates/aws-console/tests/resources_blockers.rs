//! Delete with dependents: what stands in the way of a delete (`GET …/blockers`) and the delete that clears it
//! (`DELETE …?clear=yes`), each call the service's own and the console's, against real services on a real bus.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
#[path = "resources/blocked.rs"]
mod blocked;
#[path = "resources/code.rs"]
mod code;
mod support;
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use blocked::{BASE, names, put, titles};
use serde_json::json;
use world::{encode, world};

#[tokio::test]
async fn an_event_bus_and_a_rule_are_cleared_of_their_rules_and_targets() {
    let w = world("blockers-events", Mode::Enforce, &[]);
    w.app("sqs", "CreateQueue", &json!({"QueueName": "inbox"}))
        .await;
    w.app("events", "CreateEventBus", &json!({"Name": "larder"}))
        .await;
    for rule in ["a", "b"] {
        w.app(
            "events",
            "PutRule",
            &json!({"Name": rule, "EventBusName": "larder", "EventPattern": "{\"source\":[\"x\"]}"}),
        )
        .await;
        w.app(
            "events",
            "PutTargets",
            &json!({"Rule": rule, "EventBusName": "larder", "Targets": [
                {"Id": "t1", "Arn": "arn:aws:sqs:us-east-1:000000000000:inbox"},
                {"Id": "t2", "Arn": "arn:aws:sqs:us-east-1:000000000000:inbox"}]}),
        )
        .await;
    }
    let rule = format!("{BASE}/eventbridge/rules/{}", encode("larder/a"));
    let (_, b) = w.get(&format!("{rule}/blockers")).await;
    assert_eq!(titles(&b), ["Targets"]);
    assert_eq!(names(&b, "Targets"), ["t1", "t2"]);
    let (status, refused) = w.send("DELETE", &rule, &json!({})).await;
    assert!(status >= 400, "{status}: {refused}");
    let (status, done) = w
        .send("DELETE", &format!("{rule}?clear=yes"), &json!({}))
        .await;
    assert_eq!(status, 200, "{done}");
    assert_eq!(done["cleared"], json!(["Removed 2 targets"]));
    // The bus: its remaining rule, and that rule's targets, go before it.
    let (_, b) = w
        .get(&format!("{BASE}/eventbridge/buses/larder/blockers"))
        .await;
    assert_eq!(titles(&b), ["Rules"]);
    assert_eq!(names(&b, "Rules"), ["b"]);
    let (status, done) = w
        .send(
            "DELETE",
            &format!("{BASE}/eventbridge/buses/larder?clear=yes"),
            &json!({}),
        )
        .await;
    assert_eq!(status, 200, "{done}");
    assert_eq!(
        done["cleared"],
        json!(["Removed 2 targets", "Deleted rule b"])
    );
    let (_, buses) = w.get(&format!("{BASE}/eventbridge/buses")).await;
    assert!(
        !buses["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["name"] == "larder"),
        "{buses}"
    );
}

#[tokio::test]
async fn a_bucket_is_emptied_of_objects_versions_and_markers_before_it_is_deleted() {
    let w = world("blockers-s3", Mode::Enforce, &[]);
    w.app("s3", "CreateBucket", &json!({"Bucket": "images"}))
        .await;
    for key in ["a.txt", "b/c.txt"] {
        put(&w, "images", key, "x").await;
    }
    let (_, b) = w.get(&format!("{BASE}/s3/buckets/images/blockers")).await;
    assert_eq!(titles(&b), ["Objects"]);
    assert_eq!(names(&b, "Objects"), ["a.txt", "b/c.txt"]);
    assert_eq!(b["words"], "images");
    let (status, refused) = w
        .send("DELETE", &format!("{BASE}/s3/buckets/images"), &json!({}))
        .await;
    assert_eq!(
        (status, refused["code"].as_str()),
        (409, Some("BucketNotEmpty")),
        "{refused}"
    );
    // A versioned bucket: versions and a delete marker.
    w.app("s3", "CreateBucket", &json!({"Bucket": "ledger"}))
        .await;
    w.app(
        "s3",
        "PutBucketVersioning",
        &json!({"Bucket": "ledger", "VersioningConfiguration": {"Status": "Enabled"}}),
    )
    .await;
    for body in ["one", "two"] {
        put(&w, "ledger", "k", body).await;
    }
    w.app(
        "s3",
        "DeleteObject",
        &json!({"Bucket": "ledger", "Key": "k"}),
    )
    .await;
    let (_, b) = w.get(&format!("{BASE}/s3/buckets/ledger/blockers")).await;
    assert_eq!(titles(&b), ["Objects", "Delete markers"]);
    assert_eq!(names(&b, "Objects").len(), 2);
    let (status, done) = w
        .send(
            "DELETE",
            &format!("{BASE}/s3/buckets/ledger?clear=yes"),
            &json!({}),
        )
        .await;
    assert_eq!(status, 200, "{done}");
    assert_eq!(
        done["cleared"],
        json!(["Deleted 2 objects", "Deleted 1 delete markers"])
    );
    let (status, done) = w
        .send(
            "DELETE",
            &format!("{BASE}/s3/buckets/images?clear=yes"),
            &json!({}),
        )
        .await;
    assert_eq!(status, 200, "{done}");
    assert_eq!(done["cleared"], json!(["Deleted 2 objects"]));
    let (_, buckets) = w.get(&format!("{BASE}/s3/buckets")).await;
    assert_eq!(buckets["rows"], json!([]));
}

#[tokio::test]
async fn a_stack_says_what_its_template_retains_and_a_kind_with_nothing_in_the_way_says_so() {
    let w = world("blockers-cfn", Mode::Soft, &[]);
    let template = json!({"Resources": {
        "Data": {"Type": "AWS::SQS::Queue", "DeletionPolicy": "Retain", "Properties": {"QueueName": "keep"}},
        "Work": {"Type": "AWS::SQS::Queue", "Properties": {"QueueName": "go"}}}});
    w.app(
        "cloudformation",
        "CreateStack",
        &json!({"StackName": "larder", "TemplateBody": template.to_string()}),
    )
    .await;
    let (status, b) = w
        .get(&format!("{BASE}/cloudformation/stacks/larder/blockers"))
        .await;
    assert_eq!(status, 200, "{b}");
    assert_eq!(b["blockers"], json!([]));
    assert_eq!(
        b["kept"],
        json!(["Data (AWS::SQS::Queue) is retained: its DeletionPolicy is Retain."])
    );
    assert_eq!(b["words"], "larder");
    // A queue has nothing in the way; a kind the console cannot delete has no blockers to read.
    w.app("sqs", "CreateQueue", &json!({"QueueName": "q"}))
        .await;
    let (status, b) = w.get(&format!("{BASE}/sqs/queues/q/blockers")).await;
    assert_eq!(status, 200, "{b}");
    assert_eq!(
        (b["blockers"].clone(), b["kept"].clone()),
        (json!([]), json!([]))
    );
    let (status, b) = w
        .get(&format!("{BASE}/cloudwatch/metrics/x/blockers"))
        .await;
    assert_eq!(
        (status, b["code"].as_str()),
        (404, Some("NOT_FOUND")),
        "{b}"
    );
}

#[tokio::test]
async fn a_functions_mappings_are_listed_as_left_behind_and_not_cleared() {
    let w = world("blockers-lambda", Mode::Enforce, &[]);
    if !code::function(&w, "echo").await {
        return;
    }
    w.app("sqs", "CreateQueue", &json!({"QueueName": "inbox"}))
        .await;
    w.app(
        "lambda",
        "CreateEventSourceMapping",
        &json!({"FunctionName": "echo", "EventSourceArn": "arn:aws:sqs:us-east-1:000000000000:inbox"}),
    )
    .await;
    let (status, b) = w
        .get(&format!("{BASE}/lambda/functions/echo/blockers"))
        .await;
    assert_eq!(status, 200, "{b}");
    assert_eq!(titles(&b), ["Event source mappings"]);
    assert_eq!(names(&b, "Event source mappings"), ["inbox"]);
    let group = &b["blockers"][0];
    assert_eq!(
        (group["blocks"].clone(), group["clears"].clone()),
        (json!(false), json!(false))
    );
    let kept = b["kept"].as_array().unwrap();
    assert!(
        kept[0].as_str().unwrap().contains("/aws/lambda/echo"),
        "{b}"
    );
    assert_eq!(kept.len(), 2);
    // The delete is not stopped by them, and `clear` has nothing to clear.
    let (status, done) = w
        .send(
            "DELETE",
            &format!("{BASE}/lambda/functions/echo?clear=yes"),
            &json!({}),
        )
        .await;
    assert_eq!(status, 200, "{done}");
    assert_eq!(done["cleared"], json!([]));
}
