//! Policies: documents, managed and inline, versions, attachment, boundaries.

use serde_json::json;

use super::{READ_S3, TRUST, World};

#[test]
fn a_role_with_inline_and_managed_policies_in_the_recorded_order() {
    let mut w = World::new();
    w.ok(
        "CreateRole",
        &json!({"RoleName": "r", "AssumeRolePolicyDocument": TRUST}),
    );
    w.ok(
        "PutRolePolicy",
        &json!({"RoleName": "r", "PolicyName": "inline", "PolicyDocument": READ_S3}),
    );
    assert_eq!(
        w.ok("ListRolePolicies", &json!({"RoleName": "r"}))["PolicyNames"],
        json!(["inline"])
    );
    let got = w.ok(
        "GetRolePolicy",
        &json!({"RoleName": "r", "PolicyName": "inline"}),
    );
    assert_eq!(
        (got["RoleName"].as_str(), got["PolicyName"].as_str()),
        (Some("r"), Some("inline"))
    );
    let made = w.ok(
        "CreatePolicy",
        &json!({"PolicyName": "p", "PolicyDocument": READ_S3}),
    );
    let policy = &made["Policy"];
    assert_eq!(
        (
            policy["AttachmentCount"].as_i64(),
            policy["DefaultVersionId"].as_str(),
            policy["IsAttachable"].clone()
        ),
        (Some(0), Some("v1"), json!(true))
    );
    assert!(
        policy.get("Tags").is_none(),
        "a create answer has no Tags (recorded)"
    );
    let arn = policy["Arn"].as_str().unwrap().to_owned();
    assert_eq!(arn, "arn:aws:iam::000000000000:policy/p");
    w.ok(
        "AttachRolePolicy",
        &json!({"RoleName": "r", "PolicyArn": arn}),
    );
    attached_policy_is_counted_and_protected(&mut w, &arn);
}

fn attached_policy_is_counted_and_protected(w: &mut World, arn: &str) {
    assert_eq!(
        w.ok("ListAttachedRolePolicies", &json!({"RoleName": "r"}))["AttachedPolicies"],
        json!([{"PolicyName": "p", "PolicyArn": arn}])
    );
    let after = w.ok("GetPolicy", &json!({"PolicyArn": arn}));
    assert_eq!(
        (
            after["Policy"]["AttachmentCount"].as_i64(),
            after["Policy"]["Tags"].clone()
        ),
        (Some(1), json!([]))
    );
    assert_eq!(
        w.ok(
            "GetPolicyVersion",
            &json!({"PolicyArn": arn, "VersionId": "v1"})
        )["PolicyVersion"]["IsDefaultVersion"],
        true
    );
    assert_eq!(
        w.err("DeleteRole", &json!({"RoleName": "r"})),
        (
            "DeleteConflictException".into(),
            "Cannot delete entity, must detach all policies first.".into()
        )
    );
    assert_eq!(
        w.err("DeletePolicy", &json!({"PolicyArn": arn})),
        (
            "DeleteConflictException".into(),
            "Cannot delete a policy attached to entities.".into()
        )
    );
    w.ok(
        "DetachRolePolicy",
        &json!({"RoleName": "r", "PolicyArn": arn}),
    );
    assert_eq!(
        w.err(
            "DetachRolePolicy",
            &json!({"RoleName": "r", "PolicyArn": arn})
        )
        .1,
        format!("Policy {arn} was not found.")
    );
    assert_eq!(
        w.err(
            "AttachRolePolicy",
            &json!({"RoleName": "absent", "PolicyArn": arn})
        )
        .1,
        "The role with name absent cannot be found."
    );
}

#[test]
fn policy_versions_are_listed_newest_first_and_the_default_cannot_go() {
    let mut w = World::new();
    let arn = w.ok(
        "CreatePolicy",
        &json!({"PolicyName": "p", "PolicyDocument": READ_S3}),
    )["Policy"]["Arn"]
        .as_str()
        .unwrap()
        .to_owned();
    let v2 = w.ok("CreatePolicyVersion", &json!({"PolicyArn": arn, "PolicyDocument": READ_S3.replace("GetObject", "PutObject"), "SetAsDefault": true}));
    assert_eq!(
        (
            v2["PolicyVersion"]["VersionId"].as_str(),
            v2["PolicyVersion"]["IsDefaultVersion"].clone()
        ),
        (Some("v2"), json!(true))
    );
    let ids: Vec<_> = w.ok("ListPolicyVersions", &json!({"PolicyArn": arn}))["Versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["VersionId"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(ids, ["v2", "v1"]);
    assert_eq!(
        w.err(
            "DeletePolicyVersion",
            &json!({"PolicyArn": arn, "VersionId": "v2"})
        ),
        (
            "DeleteConflictException".into(),
            "Cannot delete the default version of a policy.".into()
        )
    );
    w.ok(
        "SetDefaultPolicyVersion",
        &json!({"PolicyArn": arn, "VersionId": "v1"}),
    );
    w.ok(
        "DeletePolicyVersion",
        &json!({"PolicyArn": arn, "VersionId": "v2"}),
    );
    for _ in 0..4 {
        w.ok(
            "CreatePolicyVersion",
            &json!({"PolicyArn": arn, "PolicyDocument": READ_S3}),
        );
    }
    assert_eq!(
        w.err(
            "CreatePolicyVersion",
            &json!({"PolicyArn": arn, "PolicyDocument": READ_S3})
        )
        .0,
        "LimitExceededException"
    );
}

#[test]
fn documents_are_checked_when_they_are_stored() {
    let mut w = World::new();
    w.ok("CreateUser", &json!({"UserName": "u"}));
    assert_eq!(
        w.err(
            "CreatePolicy",
            &json!({"PolicyName": "p", "PolicyDocument": "{not json"})
        ),
        (
            "MalformedPolicyDocumentException".into(),
            "Syntax errors in policy.".into()
        )
    );
    assert_eq!(
        w.err(
            "PutUserPolicy",
            &json!({"UserName": "u", "PolicyName": "p", "PolicyDocument": TRUST})
        ),
        (
            "MalformedPolicyDocumentException".into(),
            "Policy document should not specify a principal.".into()
        )
    );
    w.ok(
        "CreateRole",
        &json!({"RoleName": "r", "AssumeRolePolicyDocument": TRUST}),
    );
    assert_eq!(
        w.err(
            "UpdateAssumeRolePolicy",
            &json!({"RoleName": "r", "PolicyDocument": READ_S3})
        )
        .1,
        "Has prohibited field Resource"
    );
    w.ok(
        "UpdateAssumeRolePolicy",
        &json!({"RoleName": "r", "PolicyDocument": TRUST.replace("lambda", "states")}),
    );
}

#[test]
fn an_aws_managed_policy_attaches_and_a_boundary_must_exist() {
    let mut w = World::new();
    w.ok(
        "CreateRole",
        &json!({"RoleName": "r", "AssumeRolePolicyDocument": TRUST}),
    );
    let aws = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole";
    w.ok(
        "AttachRolePolicy",
        &json!({"RoleName": "r", "PolicyArn": aws}),
    );
    let listed = w.ok("ListAttachedRolePolicies", &json!({"RoleName": "r"}));
    assert_eq!(
        listed["AttachedPolicies"][0]["PolicyName"],
        "AWSLambdaBasicExecutionRole"
    );
    assert_eq!(
        w.ok("ListPolicies", &json!({"Scope": "AWS"}))["Policies"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        w.ok("ListPolicies", &json!({"Scope": "Local"}))["Policies"],
        json!([])
    );
    assert_eq!(w.err("PutRolePermissionsBoundary", &json!({"RoleName": "r", "PermissionsBoundary": "arn:aws:iam::000000000000:policy/nope"})).0, "NoSuchEntityException");
    let boundary = w.ok(
        "CreatePolicy",
        &json!({"PolicyName": "b", "PolicyDocument": READ_S3}),
    )["Policy"]["Arn"]
        .as_str()
        .unwrap()
        .to_owned();
    w.ok(
        "PutRolePermissionsBoundary",
        &json!({"RoleName": "r", "PermissionsBoundary": boundary}),
    );
    assert_eq!(
        w.ok("GetRole", &json!({"RoleName": "r"}))["Role"]["PermissionsBoundary"]["PermissionsBoundaryArn"],
        boundary.as_str()
    );
    assert_eq!(
        w.err("DeletePolicy", &json!({"PolicyArn": boundary})).0,
        "DeleteConflictException",
        "a boundary in use"
    );
    assert_eq!(
        w.ok("GetPolicy", &json!({"PolicyArn": boundary}))["Policy"]["PermissionsBoundaryUsageCount"],
        1
    );
}
