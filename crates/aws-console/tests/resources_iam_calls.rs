//! The account alias, and what an IAM resource's page counts among its recent calls: a session taken of a role (STS names it), a user
//! added to a group (the group's call), a policy attached (the policy's call).
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

use aws_peers::Mode;
use serde_json::{Value, json};
use world::{World, world};

const BASE: &str = "/api/v1/resources/iam";

async fn op(w: &World, kind: &str, name: &str, input: &Value) -> (u16, Value) {
    w.send("POST", &format!("{BASE}/{kind}/op/{name}"), input)
        .await
}

async fn ok(w: &World, kind: &str, name: &str, input: Value) -> Value {
    let (status, out) = w
        .send("POST", &format!("{BASE}/{kind}/op/{name}"), &input)
        .await;
    assert_eq!(status, 200, "{kind} {name}: {out}");
    out
}

fn trust(principal: &Value) -> String {
    json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Principal": principal, "Action": "sts:AssumeRole"}]})
        .to_string()
}

#[tokio::test]
async fn a_role_page_counts_the_sessions_taken_of_it_among_its_calls() {
    let w = world("iam-ops-assumed", Mode::Soft, &[]);
    for name in ["report-job", "other"] {
        ok(
            &w,
            "roles",
            "CreateRole",
            json!({"RoleName": name, "AssumeRolePolicyDocument": trust(&json!({"AWS": "arn:aws:iam::000000000000:root"}))}),
        )
        .await;
    }
    w.app(
        "sts",
        "AssumeRole",
        &json!({"RoleArn": "arn:aws:iam::000000000000:role/report-job", "RoleSessionName": "report-job-py"}),
    )
    .await;
    let (status, role) = w.get(&format!("{BASE}/roles/report-job")).await;
    assert_eq!(status, 200, "{role}");
    let acted: Vec<(&str, &str)> = role["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["caller"].as_str().unwrap(), c["action"].as_str().unwrap()))
        .filter(|(caller, _)| *caller == "app")
        .collect();
    assert_eq!(acted, vec![("app", "AssumeRole")], "{role}");
    let (_, other) = w.get(&format!("{BASE}/roles/other")).await;
    assert!(
        other["calls"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["action"] != "AssumeRole"),
        "{other}"
    );
}

#[tokio::test]
async fn adding_a_user_to_a_group_is_a_call_on_the_group() {
    let w = world("iam-ops-member", Mode::Soft, &[]);
    w.app("iam", "CreateUser", &json!({"UserName": "dev"}))
        .await;
    w.app("iam", "CreateGroup", &json!({"GroupName": "developers"}))
        .await;
    w.app(
        "iam",
        "AddUserToGroup",
        &json!({"UserName": "dev", "GroupName": "developers"}),
    )
    .await;
    let acted = |detail: &Value| -> Vec<String> {
        detail["calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["caller"] == "app")
            .map(|c| c["action"].as_str().unwrap().to_owned())
            .collect()
    };
    let (_, group) = w.get(&format!("{BASE}/groups/developers")).await;
    assert!(
        acted(&group).contains(&"AddUserToGroup".to_owned()),
        "{group}"
    );
    let (_, user) = w.get(&format!("{BASE}/users/dev")).await;
    assert!(
        !acted(&user).contains(&"AddUserToGroup".to_owned()),
        "{user}"
    );
}

#[tokio::test]
async fn attaching_a_policy_is_a_call_on_the_policy() {
    let w = world("iam-ops-attach", Mode::Soft, &[]);
    let made = w
        .app(
            "iam",
            "CreatePolicy",
            &json!({"PolicyName": "readonly", "PolicyDocument": json!({"Version": "2012-10-17",
                "Statement": [{"Effect": "Allow", "Action": "s3:GetObject", "Resource": "*"}]}).to_string()}),
        )
        .await;
    let arn = made["Policy"]["Arn"].as_str().unwrap().to_owned();
    w.app(
        "iam",
        "CreateRole",
        &json!({"RoleName": "report-job", "AssumeRolePolicyDocument": trust(&json!({"Service": "lambda.amazonaws.com"}))}),
    )
    .await;
    w.app(
        "iam",
        "AttachRolePolicy",
        &json!({"RoleName": "report-job", "PolicyArn": arn}),
    )
    .await;
    let (_, policy) = w
        .get(&format!(
            "{BASE}/policies/{}",
            arn.replace(':', "%3A").replace('/', "%2F")
        ))
        .await;
    let actions: Vec<&str> = policy["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["caller"] == "app")
        .map(|c| c["action"].as_str().unwrap())
        .collect();
    assert!(actions.contains(&"AttachRolePolicy"), "{policy}");
}

#[tokio::test]
async fn the_account_alias_is_the_users_kinds_and_a_kind_makes_only_what_it_lists() {
    let w = world("iam-ops-alias", Mode::Soft, &[]);
    ok(
        &w,
        "users",
        "CreateAccountAlias",
        json!({"AccountAlias": "larder-dev"}),
    )
    .await;
    let aliases = ok(&w, "users", "ListAccountAliases", json!({})).await;
    assert_eq!(aliases["AccountAliases"], json!(["larder-dev"]));
    // An account has one alias: a second is IAM's refusal, so a new alias replaces the old by deleting it first.
    let (status, again) = op(
        &w,
        "users",
        "CreateAccountAlias",
        &json!({"AccountAlias": "larder-prod"}),
    )
    .await;
    assert_eq!(
        (status, again["code"].as_str()),
        (409, Some("EntityAlreadyExists")),
        "{again}"
    );
    ok(
        &w,
        "users",
        "DeleteAccountAlias",
        json!({"AccountAlias": "larder-dev"}),
    )
    .await;
    ok(
        &w,
        "users",
        "CreateAccountAlias",
        json!({"AccountAlias": "larder-prod"}),
    )
    .await;
    let aliases = ok(&w, "users", "ListAccountAliases", json!({})).await;
    assert_eq!(aliases["AccountAliases"], json!(["larder-prod"]));
    ok(
        &w,
        "users",
        "DeleteAccountAlias",
        json!({"AccountAlias": "larder-prod"}),
    )
    .await;
    // What a kind does not list is not made: a delete, another kind's call, the account's calls on the roles.
    for (kind, name) in [
        ("users", "DeleteUser"),
        ("roles", "DeleteRole"),
        ("groups", "DeleteGroup"),
        ("policies", "DeletePolicy"),
        ("roles", "GetAccountSummary"),
        ("groups", "CreateAccessKey"),
        ("policies", "AssumeRole"),
    ] {
        let (status, refused) = op(&w, kind, name, &json!({})).await;
        assert_eq!(
            (status, refused["code"].as_str()),
            (404, Some("NOT_IN_PAGE")),
            "{kind} {name}: {refused}"
        );
    }
}
