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
mod support;
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use blocked::{BASE, doc, names, policy, role, titles};
use serde_json::json;
use support::{keys, ts_keys};
use world::{encode, world};

#[tokio::test]
async fn a_role_with_policies_is_cleared_and_then_deleted() {
    let w = world("blockers-role", Mode::Enforce, &[]);
    role(&w, "runner").await;
    let arn = policy(&w, "reader").await;
    w.app(
        "iam",
        "AttachRolePolicy",
        &json!({"RoleName": "runner", "PolicyArn": arn}),
    )
    .await;
    w.app(
        "iam",
        "PutRolePolicy",
        &json!({"RoleName": "runner", "PolicyName": "inline", "PolicyDocument": doc()}),
    )
    .await;
    w.app(
        "iam",
        "CreateInstanceProfile",
        &json!({"InstanceProfileName": "ip"}),
    )
    .await;
    w.app(
        "iam",
        "AddRoleToInstanceProfile",
        &json!({"InstanceProfileName": "ip", "RoleName": "runner"}),
    )
    .await;
    let (status, b) = w.get(&format!("{BASE}/iam/roles/runner/blockers")).await;
    assert_eq!(status, 200, "{b}");
    assert_eq!(keys(&b), ts_keys("Blockers"));
    assert_eq!(
        titles(&b),
        ["Attached policies", "Inline policies", "Instance profiles"]
    );
    assert_eq!(names(&b, "Attached policies"), ["reader"]);
    let attached = &b["blockers"][0];
    assert_eq!(keys(attached), ts_keys("BlockerGroup"));
    assert_eq!(attached["clears"], true);
    assert_eq!(attached["blocks"], true);
    assert_eq!(attached["more"], false);
    assert_eq!(keys(&attached["items"][0]), ts_keys("BlockerItem"));
    assert_eq!(
        attached["items"][0]["link"],
        json!({"service": "iam", "kind": "policies", "name": arn})
    );
    assert_eq!(b["words"], "runner");
    assert_eq!(names(&b, "Inline policies"), ["inline"]);
    assert_eq!(names(&b, "Instance profiles"), ["ip"]);
    // A plain delete is the service's own refusal.
    let (status, refused) = w
        .send("DELETE", &format!("{BASE}/iam/roles/runner"), &json!({}))
        .await;
    assert_eq!(
        (status, refused["code"].as_str()),
        (400, Some("DeleteConflictException")),
        "{refused}"
    );
    // Clearing detaches, deletes and removes, then deletes the role.
    let (status, done) = w
        .send(
            "DELETE",
            &format!("{BASE}/iam/roles/runner?clear=yes"),
            &json!({}),
        )
        .await;
    assert_eq!(status, 200, "{done}");
    assert_eq!(keys(&done), ts_keys("Deleted"));
    assert_eq!(
        done["cleared"],
        json!([
            "Detached reader",
            "Deleted inline policy inline",
            "Removed from instance profile ip"
        ])
    );
    let (status, _) = w.get(&format!("{BASE}/iam/roles/runner")).await;
    assert_ne!(status, 200);
    // The policy itself stays, now attached to nothing.
    let (_, p) = w
        .get(&format!("{BASE}/iam/policies/{}", encode(&arn)))
        .await;
    assert_eq!(world::field(&p, "Attached to")["value"], "0");
    let live = w.live().join("\n");
    assert!(live.contains("console iam DetachRolePolicy"), "{live}");
}

#[tokio::test]
async fn a_user_a_group_and_a_policy_are_cleared_of_what_they_hold() {
    let w = world("blockers-iam", Mode::Enforce, &[]);
    let arn = policy(&w, "reader").await;
    w.app("iam", "CreateUser", &json!({"UserName": "ci"})).await;
    w.app("iam", "CreateGroup", &json!({"GroupName": "devs"}))
        .await;
    w.app(
        "iam",
        "AddUserToGroup",
        &json!({"UserName": "ci", "GroupName": "devs"}),
    )
    .await;
    w.app(
        "iam",
        "AttachUserPolicy",
        &json!({"UserName": "ci", "PolicyArn": arn}),
    )
    .await;
    w.app(
        "iam",
        "AttachGroupPolicy",
        &json!({"GroupName": "devs", "PolicyArn": arn}),
    )
    .await;
    w.app(
        "iam",
        "PutUserPolicy",
        &json!({"UserName": "ci", "PolicyName": "inline", "PolicyDocument": doc()}),
    )
    .await;
    let key = w
        .app("iam", "CreateAccessKey", &json!({"UserName": "ci"}))
        .await;
    let key_id = key["AccessKey"]["AccessKeyId"].as_str().unwrap().to_owned();
    // The policy: attached to a user and a group, and a second version beside its default.
    w.app(
        "iam",
        "CreatePolicyVersion",
        &json!({"PolicyArn": arn, "PolicyDocument": doc(), "SetAsDefault": false}),
    )
    .await;
    let (_, b) = w
        .get(&format!("{BASE}/iam/policies/{}/blockers", encode(&arn)))
        .await;
    assert_eq!(titles(&b), ["Users", "Groups", "Other versions"]);
    assert_eq!(names(&b, "Users"), ["ci"]);
    assert_eq!(names(&b, "Other versions"), ["v2"]);
    let (_, b) = w.get(&format!("{BASE}/iam/users/ci/blockers")).await;
    assert_eq!(
        titles(&b),
        [
            "Attached policies",
            "Inline policies",
            "Access keys",
            "Groups"
        ]
    );
    assert_eq!(names(&b, "Access keys"), std::slice::from_ref(&key_id));
    let (_, b) = w.get(&format!("{BASE}/iam/groups/devs/blockers")).await;
    assert_eq!(titles(&b), ["Attached policies", "Members"]);
    assert_eq!(names(&b, "Members"), ["ci"]);
    // The user goes with its key, policies and membership.
    let (status, done) = w
        .send(
            "DELETE",
            &format!("{BASE}/iam/users/ci?clear=yes"),
            &json!({}),
        )
        .await;
    assert_eq!(status, 200, "{done}");
    let cleared = done["cleared"].as_array().unwrap();
    assert_eq!(cleared.len(), 4, "{done}");
    assert!(cleared.contains(&json!(format!("Deleted access key {key_id}"))));
    assert!(cleared.contains(&json!("Removed from group devs")));
    // The group, now empty of members, still has its policy; the policy goes last, with everything attached to it.
    let (status, done) = w
        .send(
            "DELETE",
            &format!("{BASE}/iam/groups/devs?clear=yes"),
            &json!({}),
        )
        .await;
    assert_eq!(status, 200, "{done}");
    assert_eq!(done["cleared"], json!(["Detached reader"]));
    w.app("iam", "CreateRole", &json!({"RoleName": "r2", "AssumeRolePolicyDocument": json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Principal": {"Service": "lambda.amazonaws.com"}, "Action": "sts:AssumeRole"}]}).to_string()}))
        .await;
    w.app(
        "iam",
        "AttachRolePolicy",
        &json!({"RoleName": "r2", "PolicyArn": arn}),
    )
    .await;
    let (status, done) = w
        .send(
            "DELETE",
            &format!("{BASE}/iam/policies/{}?clear=yes", encode(&arn)),
            &json!({}),
        )
        .await;
    assert_eq!(status, 200, "{done}");
    assert_eq!(
        done["cleared"],
        json!(["Detached from role r2", "Deleted version v2"])
    );
}
