//! `GetAccountAuthorizationDetails` and `AssumeRoot`.

use serde_json::json;

use super::{READ_S3, TRUST, World};

/// An account with a user in a group, a role with a managed policy and an instance profile.
fn populated() -> (World, serde_json::Value) {
    let mut w = World::new();
    w.ok(
        "CreateUser",
        &json!({"UserName": "alice", "Tags": [{"Key": "team", "Value": "blue"}]}),
    );
    w.ok("CreateGroup", &json!({"GroupName": "ops"}));
    w.ok(
        "AddUserToGroup",
        &json!({"UserName": "alice", "GroupName": "ops"}),
    );
    w.ok(
        "PutUserPolicy",
        &json!({"UserName": "alice", "PolicyName": "inline", "PolicyDocument": READ_S3}),
    );
    w.ok(
        "CreateRole",
        &json!({"RoleName": "r", "AssumeRolePolicyDocument": TRUST}),
    );
    let policy = w.ok(
        "CreatePolicy",
        &json!({"PolicyName": "managed", "PolicyDocument": READ_S3}),
    );
    let arn = policy["Policy"]["Arn"].clone();
    w.ok(
        "AttachRolePolicy",
        &json!({"RoleName": "r", "PolicyArn": arn}),
    );
    w.ok(
        "CreateInstanceProfile",
        &json!({"InstanceProfileName": "ip"}),
    );
    w.ok(
        "AddRoleToInstanceProfile",
        &json!({"InstanceProfileName": "ip", "RoleName": "r"}),
    );

    (w, arn)
}

#[test]
fn the_dump_lists_every_principal_with_its_policies() {
    let (mut w, arn) = populated();
    let all = w.ok("GetAccountAuthorizationDetails", &json!({}));
    let user = &all["UserDetailList"][0];
    assert_eq!(user["UserName"], "alice");
    assert_eq!(user["GroupList"], json!(["ops"]));
    assert_eq!(user["UserPolicyList"][0]["PolicyName"], "inline");
    assert!(
        user["UserPolicyList"][0]["PolicyDocument"]
            .as_str()
            .unwrap()
            .starts_with("%7B"),
        "documents are URL-encoded"
    );
    assert_eq!(user["Tags"][0]["Key"], "team");
    assert_eq!(all["GroupDetailList"][0]["GroupName"], "ops");
    let role = &all["RoleDetailList"][0];
    assert_eq!(role["AttachedManagedPolicies"][0]["PolicyArn"], arn);
    assert_eq!(role["InstanceProfileList"][0]["InstanceProfileName"], "ip");
    let managed = &all["Policies"][0];
    assert_eq!(managed["AttachmentCount"], 1);
    assert_eq!(managed["PolicyVersionList"][0]["IsDefaultVersion"], true);
    assert_eq!(all["IsTruncated"], false);
}

#[test]
fn a_filter_narrows_the_dump_and_a_small_page_continues_from_its_marker() {
    let (mut w, _) = populated();
    let roles = w.ok(
        "GetAccountAuthorizationDetails",
        &json!({"Filter": ["Role"]}),
    );
    assert_eq!(roles["UserDetailList"], json!([]));
    assert_eq!(roles["RoleDetailList"].as_array().unwrap().len(), 1);
    let first = w.ok("GetAccountAuthorizationDetails", &json!({"MaxItems": 2}));
    assert_eq!(first["IsTruncated"], true);
    assert_eq!(
        first["UserDetailList"].as_array().unwrap().len()
            + first["GroupDetailList"].as_array().unwrap().len(),
        2
    );
    let rest = w.ok(
        "GetAccountAuthorizationDetails",
        &json!({"Marker": first["Marker"]}),
    );
    assert_eq!(rest["IsTruncated"], false);
    assert_eq!(rest["UserDetailList"], json!([]));
    assert_eq!(rest["RoleDetailList"].as_array().unwrap().len(), 1);
}

#[test]
fn assume_root_hands_out_short_credentials_for_the_target_account() {
    let mut w = World::new();
    let out = w.sts_ok(
        "AssumeRoot",
        &json!({"TargetPrincipal": "111122223333", "TaskPolicyArn": {"arn": "arn:aws:iam::aws:policy/root-task/IAMAuditRootUserCredentials"}}),
    );
    assert!(
        out["Credentials"]["AccessKeyId"]
            .as_str()
            .unwrap()
            .starts_with("ASIA")
    );
    assert!(out["Credentials"]["SessionToken"].as_str().is_some());
    let long = w.sts_err(
        "AssumeRoot",
        &json!({"TargetPrincipal": "111122223333", "TaskPolicyArn": {"arn": "arn:aws:iam::aws:policy/root-task/IAMAuditRootUserCredentials"}, "DurationSeconds": 901}),
    );
    assert_eq!(long.0, "ValidationError");
}
