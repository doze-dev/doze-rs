//! The policies a user or role holds, as Live's Explain panel reads them: inline ones with their documents (so Add to policy can
//! edit one instead of making another), managed ones attached, and the groups it did not read.
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
use world::world;

fn allow(action: &str, resource: &str) -> String {
    json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Action": action, "Resource": resource}]}).to_string()
}

#[tokio::test]
async fn a_user_s_inline_and_attached_policies_are_read_with_their_documents() {
    let w = world("held-policies", Mode::Soft, &[]);
    w.app("iam", "CreateUser", &json!({"UserName": "report-job"}))
        .await;
    w.app(
        "iam",
        "PutUserPolicy",
        &json!({"UserName": "report-job", "PolicyName": "report-job-readonly", "PolicyDocument": allow("s3:GetObject", "arn:aws:s3:::reports/*")}),
    )
    .await;
    let made = w
        .app(
            "iam",
            "CreatePolicy",
            &json!({"PolicyName": "audit", "PolicyDocument": allow("logs:Get*", "*")}),
        )
        .await;
    let arn = made["Policy"]["Arn"].as_str().unwrap().to_owned();
    w.app(
        "iam",
        "AttachUserPolicy",
        &json!({"UserName": "report-job", "PolicyArn": arn}),
    )
    .await;
    w.app("iam", "CreateGroup", &json!({"GroupName": "readers"}))
        .await;
    w.app(
        "iam",
        "AddUserToGroup",
        &json!({"UserName": "report-job", "GroupName": "readers"}),
    )
    .await;

    let (status, held) = w
        .get("/api/v1/access/policies?kind=user&name=report-job")
        .await;
    assert_eq!(status, 200, "{held}");
    assert_eq!(keys(&held), ts_keys("HeldPolicies"));
    let policies = held["policies"].as_array().unwrap();
    assert_eq!(policies.len(), 2, "{held}");
    assert_eq!(keys(&policies[0]), ts_keys("HeldPolicy"));
    let inline = policies.iter().find(|p| p["inline"] == true).unwrap();
    assert_eq!(inline["name"], "report-job-readonly");
    let doc: Value = serde_json::from_str(inline["document"].as_str().unwrap()).unwrap();
    assert_eq!(doc["Statement"][0]["Resource"], "arn:aws:s3:::reports/*");
    let managed = policies.iter().find(|p| p["inline"] == false).unwrap();
    assert_eq!(managed["name"], "audit");
    assert_eq!(managed["arn"], arn);
    assert!(managed["document"].as_str().unwrap().contains("logs:Get*"));
    assert_eq!(held["groups"], json!(["readers"]));
}

#[tokio::test]
async fn what_is_not_a_user_or_a_role_is_refused() {
    let w = world("held-policies-bad", Mode::Soft, &[]);
    for bad in [
        "/api/v1/access/policies",
        "/api/v1/access/policies?kind=group&name=g",
        "/api/v1/access/policies?kind=user",
    ] {
        let (status, body) = w.get(bad).await;
        assert_eq!(status, 400, "{bad}: {body}");
    }
}
