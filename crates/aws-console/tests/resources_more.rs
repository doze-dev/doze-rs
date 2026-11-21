//! The Resources API's second part against real services on a real bus: IAM, API Gateway and CloudFormation, each made,
//! listed, opened (with its related lists) and deleted as the caller `console`; and what part one left: the console's own calls
//! set apart, a bucket's region and its absent configuration, AWS managed keys set aside, a subscription added from its topic.
//! Logs, Kinesis, CloudWatch, EventBridge, Scheduler and Step Functions are in `resources_flow.rs`.
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
use serde_json::{Value, json};
use support::{keys, ts_keys};
use world::{World, encode, field, firsts, sub, world};

#[tokio::test]
async fn a_role_is_made_from_its_trust_template_and_a_user_never_shows_a_secret() {
    let w = world("iam", Mode::Enforce, &[]);
    let (_, list) = w.get("/api/v1/resources/iam/roles").await;
    let trust = list["create"]["inputs"][1]["value"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(trust.contains("lambda.amazonaws.com"), "{trust}");
    let form = json!({"RoleName": "orders-handler", "AssumeRolePolicyDocument": trust, "Description": "runs orders"});
    w.round_trip("/api/v1/resources/iam/roles", &form, "orders-handler")
        .await;
    let (_, detail) = w.get("/api/v1/resources/iam/roles/orders-handler").await;
    let doc: Value =
        serde_json::from_str(field(&detail, "Trust policy")["value"].as_str().unwrap()).unwrap();
    assert_eq!(
        doc["Statement"][0]["Principal"]["Service"],
        "lambda.amazonaws.com"
    );
    assert_eq!(field(&detail, "Last used")["value"], "Never");
    w.delete("/api/v1/resources/iam/roles", "orders-handler")
        .await;

    w.round_trip(
        "/api/v1/resources/iam/users",
        &json!({"UserName": "ci"}),
        "ci",
    )
    .await;
    let key = w
        .app("iam", "CreateAccessKey", &json!({"UserName": "ci"}))
        .await;
    let secret = key["AccessKey"]["SecretAccessKey"].as_str().unwrap();
    let id = key["AccessKey"]["AccessKeyId"].as_str().unwrap();
    let (_, user) = w.get("/api/v1/resources/iam/users/ci").await;
    assert_eq!(firsts(sub(&user, "Access keys")), [id]);
    assert!(!user.to_string().contains(secret), "{user}");
    // A user with a key is refused by IAM itself, in its words.
    let (status, refused) = w
        .send("DELETE", "/api/v1/resources/iam/users/ci", &json!({}))
        .await;
    assert!(
        status >= 400
            && refused["code"]
                .as_str()
                .unwrap()
                .starts_with("DeleteConflict"),
        "{refused}"
    );
}

#[tokio::test]
async fn a_policy_shows_what_it_is_attached_to_and_a_group_its_users() {
    let w = world("iam-policies", Mode::Enforce, &[]);
    w.app("iam", "CreateUser", &json!({"UserName": "ci"})).await;
    let (_, list) = w.get("/api/v1/resources/iam/policies").await;
    let document = list["create"]["inputs"][1]["value"]
        .as_str()
        .unwrap()
        .to_owned();
    let form = json!({"PolicyName": "orders-read", "PolicyDocument": document});
    let arn = w
        .round_trip("/api/v1/resources/iam/policies", &form, "orders-read")
        .await;
    assert_eq!(arn, "arn:aws:iam::000000000000:policy/orders-read");
    w.app(
        "iam",
        "AttachUserPolicy",
        &json!({"UserName": "ci", "PolicyArn": arn}),
    )
    .await;
    let (_, policy) = w
        .get(&format!("/api/v1/resources/iam/policies/{}", encode(&arn)))
        .await;
    assert!(
        field(&policy, "Document")["value"]
            .as_str()
            .unwrap()
            .contains("s3:GetObject"),
        "{policy}"
    );
    assert_eq!(sub(&policy, "Users")["rows"][0]["link"]["name"], "ci");
    let (_, user) = w.get("/api/v1/resources/iam/users/ci").await;
    assert_eq!(
        sub(&user, "Attached policies")["rows"][0]["link"],
        json!({"service": "iam", "kind": "policies", "name": arn})
    );
    w.round_trip(
        "/api/v1/resources/iam/groups",
        &json!({"GroupName": "devs"}),
        "devs",
    )
    .await;
    w.app(
        "iam",
        "AddUserToGroup",
        &json!({"UserName": "ci", "GroupName": "devs"}),
    )
    .await;
    let (_, group) = w.get("/api/v1/resources/iam/groups/devs").await;
    assert_eq!(firsts(sub(&group, "Users")), ["ci"]);
}

#[tokio::test]
async fn apis_of_both_kinds_show_their_stages_and_a_stack_its_resources_and_outputs() {
    let w = world("apis", Mode::Soft, &[]);
    let id = w
        .round_trip(
            "/api/v1/resources/apigateway/restapis",
            &json!({"name": "orders-api", "endpointType": "REGIONAL"}),
            "orders-api",
        )
        .await;
    let (_, detail) = w
        .get(&format!("/api/v1/resources/apigateway/restapis/{id}"))
        .await;
    assert_eq!(firsts(sub(&detail, "Resources")), ["/"]);
    assert_eq!(sub(&detail, "Stages")["error"], Value::Null);
    w.delete("/api/v1/resources/apigateway/restapis", &id).await;
    let id = w
        .round_trip(
            "/api/v1/resources/apigateway/apis",
            &json!({"Name": "orders-http", "ProtocolType": "HTTP"}),
            "orders-http",
        )
        .await;
    let (_, detail) = w
        .get(&format!("/api/v1/resources/apigateway/apis/{id}"))
        .await;
    assert_eq!(field(&detail, "Protocol")["value"], "HTTP");
    w.delete("/api/v1/resources/apigateway/apis", &id).await;
}

#[tokio::test]
async fn a_stack_made_from_a_template_shows_its_resources_outputs_and_events() {
    let w = world("stack", Mode::Soft, &[]);
    let template = json!({"Resources": {"Inbox": {"Type": "AWS::SQS::Queue", "Properties": {"QueueName": "stack-inbox"}}},
        "Outputs": {"InboxUrl": {"Value": {"Ref": "Inbox"}, "Description": "the queue"}}});
    w.app(
        "cloudformation",
        "CreateStack",
        &json!({"StackName": "larder", "TemplateBody": template.to_string()}),
    )
    .await;
    let mut detail = Value::Null;
    for _ in 0..100 {
        detail = w
            .get("/api/v1/resources/cloudformation/stacks/larder")
            .await
            .1;
        if field(&detail, "Status")["value"] == "CREATE_COMPLETE" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(
        field(&detail, "Status")["value"],
        "CREATE_COMPLETE",
        "{detail}"
    );
    assert_eq!(firsts(sub(&detail, "Resources")), ["Inbox"]);
    let outputs = sub(&detail, "Outputs");
    assert_eq!(outputs["operation"], "DescribeStacks");
    assert_eq!(outputs["rows"][0]["cells"][0], "InboxUrl");
    assert!(
        outputs["rows"][0]["cells"][1]
            .as_str()
            .unwrap()
            .ends_with("/stack-inbox")
    );
    // Its events are a page of their own.
    let (_, events) = w
        .send(
            "POST",
            "/api/v1/resources/cloudformation/stacks/larder/op/DescribeStackEvents",
            &json!({}),
        )
        .await;
    assert!(
        !events["StackEvents"].as_array().unwrap().is_empty(),
        "{events}"
    );
    // A stack is deleted in the background: it is DELETE_IN_PROGRESS first.
    let (status, done) = w
        .send(
            "DELETE",
            "/api/v1/resources/cloudformation/stacks/larder",
            &json!({}),
        )
        .await;
    assert_eq!((status, &done["operation"]), (200, &json!("DeleteStack")));
}

/// The calls of `detail` made by the console, and by others.
fn callers(detail: &Value) -> (usize, usize) {
    let calls = detail["calls"].as_array().unwrap();
    let mine = calls.iter().filter(|c| c["caller"] == "console").count();
    (mine, calls.len() - mine)
}

async fn bucket_and_key_checks(w: &World) {
    w.round_trip(
        "/api/v1/resources/s3/buckets",
        &json!({"Bucket": "photos"}),
        "photos",
    )
    .await;
    let (_, detail) = w.get("/api/v1/resources/s3/buckets/photos").await;
    assert_eq!(field(&detail, "Region")["value"], "us-east-1");
    assert_eq!(field(&detail, "Bucket policy")["value"], "None");
    assert_eq!(field(&detail, "CORS rules")["value"], "None");
    w.app("s3", "HeadBucket", &json!({"Bucket": "photos"}))
        .await;
    let (_, detail) = w.get("/api/v1/resources/s3/buckets/photos").await;
    let (mine, theirs) = callers(&detail);
    assert!(mine > 0 && theirs == 1, "{mine} {theirs}");
}

#[tokio::test]
async fn the_console_sets_its_own_calls_and_aws_managed_keys_apart() {
    let w = world("apart", Mode::Soft, &[]);
    bucket_and_key_checks(&w).await;
    let form = json!({"Description": "mine", "KeySpec": "SYMMETRIC_DEFAULT", "KeyUsage": "ENCRYPT_DECRYPT"});
    let mine = w.round_trip("/api/v1/resources/kms/keys", &form, "").await;
    // A secret with no key of its own is encrypted with the AWS managed aws/secretsmanager key.
    w.app(
        "secretsmanager",
        "CreateSecret",
        &json!({"Name": "s", "SecretString": "v"}),
    )
    .await;
    let (_, list) = w.get("/api/v1/resources/kms/keys").await;
    let names: Vec<&str> = list["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, [mine.as_str()]);
    assert_eq!(keys(&list["others"]), ts_keys("SetAside"));
    assert_eq!(list["others"]["what"], "AWS managed keys");
    let managed = list["others"]["count"].as_u64().unwrap();
    let (_, all) = w.get("/api/v1/resources/kms/keys?all=yes").await;
    assert_eq!(
        all["rows"].as_array().unwrap().len(),
        1 + usize::try_from(managed).unwrap()
    );
    assert_eq!(all["all"], true);
    let (_, index) = w.get("/api/v1/resources").await;
    let kms = index["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "kms")
        .unwrap();
    assert_eq!(kms["kinds"][0]["count"], 1);
    assert_eq!(kms["kinds"][0]["others"]["count"], managed);
    assert_eq!(kms["kinds"][1]["id"], "aliases");
    // Every service but STS is described.
    for s in index["services"].as_array().unwrap() {
        assert_eq!(s["described"], s["name"] != "sts", "{s}");
    }
    // The map's lines say which are the console's.
    let (_, flows) = w.get("/api/v1/flows").await;
    let flows = flows["flows"].as_array().unwrap();
    assert!(
        flows
            .iter()
            .any(|f| f["console"] == true && f["caller"] == "console")
    );
    assert!(
        flows
            .iter()
            .any(|f| f["console"] == false && f["caller"] == "app")
    );
}

#[tokio::test]
async fn a_topic_offers_a_subscription_to_itself() {
    let w = world("add", Mode::Soft, &[]);
    w.round_trip(
        "/api/v1/resources/sns/topics",
        &json!({"Name": "orders"}),
        "orders",
    )
    .await;
    let (_, topic) = w.get("/api/v1/resources/sns/topics/orders").await;
    let add = &sub(&topic, "Subscriptions")["add"];
    assert_eq!(keys(add), ts_keys("SubAdd"));
    assert_eq!(
        (&add["kind"], &add["key"], &add["value"]),
        (
            &json!("subscriptions"),
            &json!("TopicArn"),
            &json!("arn:aws:sns:us-east-1:000000000000:orders")
        )
    );
    assert_eq!(add["create"]["operations"], json!(["Subscribe"]));
}
