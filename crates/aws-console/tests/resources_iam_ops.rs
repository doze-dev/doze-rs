//! The operations the IAM pages make without a resource bound (`POST …/iam/<kind>/op/<Operation>`): the wizards that make a user,
//! group, role and policy, the access key calls, the trust policy, the policy versions, the account's alias, and STS's
//! `AssumeRole`; each kind makes only what it lists.
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
const POWER: &str = "arn:aws:iam::aws:policy/PowerUserAccess";

async fn op(w: &World, kind: &str, name: &str, input: &Value) -> (u16, Value) {
    w.send("POST", &format!("{BASE}/{kind}/op/{name}"), input)
        .await
}

async fn ok(w: &World, kind: &str, name: &str, input: Value) -> Value {
    let (status, out) = op(w, kind, name, &input).await;
    assert_eq!(status, 200, "{kind} {name}: {out}");
    out
}

fn trust(principal: &Value) -> String {
    json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Principal": principal, "Action": "sts:AssumeRole"}]})
        .to_string()
}

#[tokio::test]
async fn a_user_is_made_with_its_tags_group_and_policy_and_its_key_is_shown_once() {
    let w = world("iam-ops-user", Mode::Soft, &[]);
    let made = ok(
        &w,
        "users",
        "CreateUser",
        json!({"UserName": "dev-intern", "Tags": [{"Key": "team", "Value": "larder"}]}),
    )
    .await;
    assert_eq!(made["User"]["UserName"], "dev-intern");
    ok(
        &w,
        "groups",
        "CreateGroup",
        json!({"GroupName": "developers"}),
    )
    .await;
    ok(
        &w,
        "groups",
        "AttachGroupPolicy",
        json!({"GroupName": "developers", "PolicyArn": POWER}),
    )
    .await;
    ok(
        &w,
        "users",
        "AddUserToGroup",
        json!({"UserName": "dev-intern", "GroupName": "developers"}),
    )
    .await;
    ok(
        &w,
        "users",
        "AttachUserPolicy",
        json!({"UserName": "dev-intern", "PolicyArn": POWER}),
    )
    .await;
    let groups = ok(
        &w,
        "users",
        "ListGroupsForUser",
        json!({"UserName": "dev-intern"}),
    )
    .await;
    assert_eq!(groups["Groups"][0]["GroupName"], "developers");
    let held = ok(
        &w,
        "groups",
        "ListAttachedGroupPolicies",
        json!({"GroupName": "developers"}),
    )
    .await;
    assert_eq!(held["AttachedPolicies"][0]["PolicyName"], "PowerUserAccess");
    let attached = ok(
        &w,
        "users",
        "ListAttachedUserPolicies",
        json!({"UserName": "dev-intern"}),
    )
    .await;
    assert_eq!(
        attached["AttachedPolicies"][0]["PolicyArn"],
        json!(POWER),
        "{attached}"
    );

    // The key: the secret is in the answer to CreateAccessKey and nowhere after it.
    let key = ok(
        &w,
        "users",
        "CreateAccessKey",
        json!({"UserName": "dev-intern"}),
    )
    .await;
    let id = key["AccessKey"]["AccessKeyId"].as_str().unwrap().to_owned();
    assert!(
        key["AccessKey"]["SecretAccessKey"]
            .as_str()
            .is_some_and(|s| s.len() >= 20)
    );
    assert_eq!(key["AccessKey"]["Status"], "Active");
    let listed = ok(
        &w,
        "users",
        "ListAccessKeys",
        json!({"UserName": "dev-intern"}),
    )
    .await;
    assert_eq!(listed["AccessKeyMetadata"][0]["AccessKeyId"], json!(id));
    assert!(listed["AccessKeyMetadata"][0]["SecretAccessKey"].is_null());
    let used = ok(
        &w,
        "users",
        "GetAccessKeyLastUsed",
        json!({"AccessKeyId": id}),
    )
    .await;
    assert_eq!(used["UserName"], "dev-intern");
    ok(
        &w,
        "users",
        "UpdateAccessKey",
        json!({"UserName": "dev-intern", "AccessKeyId": id, "Status": "Inactive"}),
    )
    .await;
    let listed = ok(
        &w,
        "users",
        "ListAccessKeys",
        json!({"UserName": "dev-intern"}),
    )
    .await;
    assert_eq!(listed["AccessKeyMetadata"][0]["Status"], "Inactive");
    ok(
        &w,
        "users",
        "DeleteAccessKey",
        json!({"UserName": "dev-intern", "AccessKeyId": id}),
    )
    .await;
    ok(
        &w,
        "users",
        "DetachUserPolicy",
        json!({"UserName": "dev-intern", "PolicyArn": POWER}),
    )
    .await;
    ok(
        &w,
        "users",
        "RemoveUserFromGroup",
        json!({"UserName": "dev-intern", "GroupName": "developers"}),
    )
    .await;
    let group = ok(&w, "groups", "GetGroup", json!({"GroupName": "developers"})).await;
    assert_eq!(group["Users"], json!([]));
    ok(
        &w,
        "groups",
        "DetachGroupPolicy",
        json!({"GroupName": "developers", "PolicyArn": POWER}),
    )
    .await;
    ok(
        &w,
        "groups",
        "AddUserToGroup",
        json!({"UserName": "dev-intern", "GroupName": "developers"}),
    )
    .await;
    ok(
        &w,
        "users",
        "ListUserPolicies",
        json!({"UserName": "dev-intern"}),
    )
    .await;
}

#[tokio::test]
async fn a_role_is_made_with_its_trust_and_policies_and_the_trust_can_be_replaced_and_assumed() {
    let w = world("iam-ops-role", Mode::Soft, &[]);
    let account = ok(&w, "users", "GetAccountSummary", json!({})).await["SummaryMap"].clone();
    assert!(account["Users"].is_number(), "{account}");
    let made = ok(
        &w,
        "roles",
        "CreateRole",
        json!({"RoleName": "report-job", "Description": "Read-only exports",
            "AssumeRolePolicyDocument": trust(&json!({"AWS": "arn:aws:iam::000000000000:root"})),
            "Tags": [{"Key": "team", "Value": "larder"}]}),
    )
    .await;
    assert_eq!(made["Role"]["RoleName"], "report-job");
    ok(
        &w,
        "roles",
        "AttachRolePolicy",
        json!({"RoleName": "report-job", "PolicyArn": POWER}),
    )
    .await;
    let held = ok(
        &w,
        "roles",
        "ListAttachedRolePolicies",
        json!({"RoleName": "report-job"}),
    )
    .await;
    assert_eq!(held["AttachedPolicies"][0]["PolicyName"], "PowerUserAccess");
    // The trust policy is replaced whole, and the new one is what the role has.
    let next = trust(&json!({"Service": "lambda.amazonaws.com"}));
    ok(
        &w,
        "roles",
        "UpdateAssumeRolePolicy",
        json!({"RoleName": "report-job", "PolicyDocument": next}),
    )
    .await;
    let (_, role) = w.get(&format!("{BASE}/roles/report-job")).await;
    assert!(
        role.to_string().contains("lambda.amazonaws.com"),
        "the role's trust is the new one: {role}"
    );
    // A document that breaks the grammar is IAM's refusal, passed on with its code.
    let (status, bad) = op(
        &w,
        "roles",
        "UpdateAssumeRolePolicy",
        &json!({"RoleName": "report-job", "PolicyDocument": "{\"Version\":\"2012-10-17\"}"}),
    )
    .await;
    assert_eq!(status, 400, "{bad}");
    assert_eq!(bad["code"], "MalformedPolicyDocument");

    // AssumeRole is STS's, made from the roles' page: the credentials come with a session token.
    let session = ok(
        &w,
        "roles",
        "AssumeRole",
        json!({"RoleArn": "arn:aws:iam::000000000000:role/report-job", "RoleSessionName": "console-report-job", "DurationSeconds": 900}),
    )
    .await;
    let credentials = &session["Credentials"];
    assert!(credentials["AccessKeyId"].as_str().is_some(), "{session}");
    assert!(credentials["SessionToken"].as_str().is_some());
    assert!(
        session["AssumedRoleUser"]["Arn"]
            .as_str()
            .unwrap()
            .ends_with("assumed-role/report-job/console-report-job")
    );
    assert!(
        w.live().iter().any(|l| l == "console sts AssumeRole"),
        "{:?}",
        w.live()
    );
    ok(
        &w,
        "roles",
        "DetachRolePolicy",
        json!({"RoleName": "report-job", "PolicyArn": POWER}),
    )
    .await;
}

#[tokio::test]
async fn a_policy_is_made_with_tags_gets_versions_and_the_account_gets_an_alias() {
    let w = world("iam-ops-policy", Mode::Soft, &[]);
    let doc = |action: &str| {
        json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Action": action, "Resource": "*"}]})
            .to_string()
    };
    let made = ok(
        &w,
        "policies",
        "CreatePolicy",
        json!({"PolicyName": "pick-lists-send-only", "PolicyDocument": doc("sqs:SendMessage"),
            "Description": "Send to pick-lists", "Tags": [{"Key": "team", "Value": "larder"}]}),
    )
    .await;
    let arn = made["Policy"]["Arn"].as_str().unwrap().to_owned();
    assert_eq!(made["Policy"]["DefaultVersionId"], "v1");
    let v2 = ok(
        &w,
        "policies",
        "CreatePolicyVersion",
        json!({"PolicyArn": arn, "PolicyDocument": doc("sqs:ReceiveMessage")}),
    )
    .await;
    assert_eq!(v2["PolicyVersion"]["VersionId"], "v2");
    ok(
        &w,
        "policies",
        "SetDefaultPolicyVersion",
        json!({"PolicyArn": arn, "VersionId": "v2"}),
    )
    .await;
    let versions = ok(
        &w,
        "policies",
        "ListPolicyVersions",
        json!({"PolicyArn": arn}),
    )
    .await;
    assert_eq!(versions["Versions"].as_array().unwrap().len(), 2);
    let got = ok(
        &w,
        "policies",
        "GetPolicyVersion",
        json!({"PolicyArn": arn, "VersionId": "v1"}),
    )
    .await;
    assert!(
        got["PolicyVersion"]["Document"]
            .to_string()
            .contains("SendMessage")
    );
    ok(
        &w,
        "policies",
        "DeletePolicyVersion",
        json!({"PolicyArn": arn, "VersionId": "v1"}),
    )
    .await;
    // Attached to a role (the policy's own page attaches it), it is listed with what it is attached to; an AWS managed
    // policy is listed once something holds it.
    ok(
        &w,
        "roles",
        "CreateRole",
        json!({"RoleName": "r", "AssumeRolePolicyDocument": trust(&json!({"Service": "lambda.amazonaws.com"}))}),
    )
    .await;
    ok(
        &w,
        "policies",
        "AttachRolePolicy",
        json!({"RoleName": "r", "PolicyArn": arn}),
    )
    .await;
    ok(
        &w,
        "policies",
        "AttachRolePolicy",
        json!({"RoleName": "r", "PolicyArn": POWER}),
    )
    .await;
    let entities = ok(
        &w,
        "policies",
        "ListEntitiesForPolicy",
        json!({"PolicyArn": arn}),
    )
    .await;
    assert_eq!(entities["PolicyRoles"][0]["RoleName"], "r");
    let aws = ok(&w, "policies", "ListPolicies", json!({"Scope": "AWS"})).await;
    assert_eq!(aws["Policies"][0]["PolicyName"], "PowerUserAccess");
    let (status, gone) = op(
        &w,
        "policies",
        "AttachUserPolicy",
        &json!({"UserName": "nobody", "PolicyArn": arn}),
    )
    .await;
    assert_eq!(
        (status, gone["code"].as_str()),
        (404, Some("NoSuchEntity")),
        "{gone}"
    );
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
