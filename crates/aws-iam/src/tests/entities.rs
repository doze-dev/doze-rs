//! Users, groups, roles, keys and instance profiles, with the answers AWS gave where they were recorded (2026-11-09).

use serde_json::json;

use super::{READ_S3, TRUST, World};

#[test]
fn a_role_is_made_read_updated_listed_and_deleted() {
    let mut w = World::new();
    let made = w.ok("CreateRole", &json!({"RoleName": "r", "AssumeRolePolicyDocument": TRUST, "Description": "d", "Tags": [{"Key": "env", "Value": "dev"}], "Path": "/apps/"}));
    let role = &made["Role"];
    assert_eq!(role["Arn"], "arn:aws:iam::000000000000:role/apps/r");
    assert!(
        role["RoleId"].as_str().unwrap().starts_with("AROA")
            && role["RoleId"].as_str().unwrap().len() == 21
    );
    assert!(
        role.get("Description").is_none() && role.get("MaxSessionDuration").is_none(),
        "a create answer has neither (recorded)"
    );
    let got = w.ok("GetRole", &json!({"RoleName": "r"}));
    assert_eq!(
        (
            got["Role"]["Description"].as_str(),
            got["Role"]["MaxSessionDuration"].as_i64()
        ),
        (Some("d"), Some(3600))
    );
    // The trust policy comes back URL-encoded, as IAM writes it.
    assert!(
        got["Role"]["AssumeRolePolicyDocument"]
            .as_str()
            .unwrap()
            .starts_with("%7B")
    );
    w.ok(
        "UpdateRole",
        &json!({"RoleName": "r", "Description": "new", "MaxSessionDuration": 7200}),
    );
    let got = w.ok("GetRole", &json!({"RoleName": "r"}));
    assert_eq!(
        (
            got["Role"]["Description"].as_str(),
            got["Role"]["MaxSessionDuration"].as_i64()
        ),
        (Some("new"), Some(7200))
    );
    assert_eq!(
        w.ok("ListRoles", &json!({"PathPrefix": "/apps/"}))["Roles"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        w.ok("ListRoles", &json!({"PathPrefix": "/other/"}))["Roles"],
        json!([])
    );
    w.ok(
        "TagRole",
        &json!({"RoleName": "r", "Tags": [{"Key": "team", "Value": "x"}]}),
    );
    w.ok("UntagRole", &json!({"RoleName": "r", "TagKeys": ["env"]}));
    assert_eq!(
        w.ok("ListRoleTags", &json!({"RoleName": "r"}))["Tags"],
        json!([{"Key": "team", "Value": "x"}])
    );
    w.ok("DeleteRole", &json!({"RoleName": "r"}));
    assert_eq!(
        w.err("GetRole", &json!({"RoleName": "r"})),
        (
            "NoSuchEntityException".into(),
            "The role with name r cannot be found.".into()
        )
    );
}

#[test]
fn refusals_use_the_words_recorded() {
    let mut w = World::new();
    w.ok(
        "CreateRole",
        &json!({"RoleName": "r", "AssumeRolePolicyDocument": TRUST}),
    );
    assert_eq!(
        w.err(
            "CreateRole",
            &json!({"RoleName": "r", "AssumeRolePolicyDocument": TRUST})
        ),
        (
            "EntityAlreadyExistsException".into(),
            "Role with name r already exists.".into()
        )
    );
    assert_eq!(
        w.err("GetUser", &json!({"UserName": "u"})).1,
        "The user with name u cannot be found."
    );
    assert_eq!(
        w.err(
            "GetRolePolicy",
            &json!({"RoleName": "r", "PolicyName": "absent"})
        )
        .1,
        "The role policy with name absent cannot be found."
    );
    assert_eq!(
        w.err(
            "CreateRole",
            &json!({"RoleName": "r2", "AssumeRolePolicyDocument": "{not json"})
        )
        .1,
        "This policy contains invalid Json"
    );
    assert_eq!(
        w.err(
            "CreateRole",
            &json!({"RoleName": "r2", "AssumeRolePolicyDocument": READ_S3})
        )
        .1,
        "Has prohibited field Resource"
    );
    let (code, message) = w.err(
        "CreateRole",
        &json!({"RoleName": "has space", "AssumeRolePolicyDocument": TRUST}),
    );
    assert_eq!(code, "ValidationError");
    assert_eq!(
        message,
        "The specified value for roleName is invalid. It must contain only alphanumeric characters and/or the following: +=,.@_-"
    );
}

#[test]
fn users_groups_and_keys() {
    let mut w = World::new();
    w.ok("CreateUser", &json!({"UserName": "u", "Path": "/apps/"}));
    w.ok("CreateGroup", &json!({"GroupName": "g"}));
    w.ok(
        "AddUserToGroup",
        &json!({"GroupName": "g", "UserName": "u"}),
    );
    assert_eq!(
        w.ok("ListGroupsForUser", &json!({"UserName": "u"}))["Groups"][0]["GroupName"],
        "g"
    );
    assert_eq!(
        w.ok("GetGroup", &json!({"GroupName": "g"}))["Users"][0]["UserName"],
        "u"
    );
    assert_eq!(
        w.err("DeleteGroup", &json!({"GroupName": "g"})).0,
        "DeleteConflictException"
    );
    let key = w.ok("CreateAccessKey", &json!({"UserName": "u"}))["AccessKey"].clone();
    assert!(
        key["AccessKeyId"].as_str().unwrap().starts_with("AKIA")
            && key["SecretAccessKey"].as_str().unwrap().len() == 40
    );
    let id = key["AccessKeyId"].as_str().unwrap();
    w.ok(
        "UpdateAccessKey",
        &json!({"UserName": "u", "AccessKeyId": id, "Status": "Inactive"}),
    );
    assert_eq!(
        w.ok("ListAccessKeys", &json!({"UserName": "u"}))["AccessKeyMetadata"][0]["Status"],
        "Inactive"
    );
    w.ok("CreateAccessKey", &json!({"UserName": "u"}));
    assert_eq!(
        w.err("CreateAccessKey", &json!({"UserName": "u"})).0,
        "LimitExceededException"
    );
    assert_eq!(
        w.err("DeleteUser", &json!({"UserName": "u"})).0,
        "DeleteConflictException"
    );
    // A rename moves the user, its keys and its place in the group.
    w.ok(
        "UpdateUser",
        &json!({"UserName": "u", "NewUserName": "v", "NewPath": "/new/"}),
    );
    assert_eq!(
        w.ok("GetUser", &json!({"UserName": "v"}))["User"]["Arn"],
        "arn:aws:iam::000000000000:user/new/v"
    );
    assert_eq!(
        w.ok("ListAccessKeys", &json!({"UserName": "v"}))["AccessKeyMetadata"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        w.ok("GetGroup", &json!({"GroupName": "g"}))["Users"][0]["UserName"],
        "v"
    );
    assert_eq!(
        w.ok("GetAccessKeyLastUsed", &json!({"AccessKeyId": id}))["AccessKeyLastUsed"]["ServiceName"],
        "N/A"
    );
}

#[test]
fn an_instance_profile_holds_one_role() {
    let mut w = World::new();
    let ec2 = TRUST.replace("lambda", "ec2");
    w.ok(
        "CreateRole",
        &json!({"RoleName": "r", "AssumeRolePolicyDocument": ec2}),
    );
    w.ok(
        "CreateRole",
        &json!({"RoleName": "r2", "AssumeRolePolicyDocument": ec2}),
    );
    let made = w.ok(
        "CreateInstanceProfile",
        &json!({"InstanceProfileName": "p"}),
    );
    assert_eq!(made["InstanceProfile"]["Roles"], json!([]));
    assert!(
        made["InstanceProfile"].get("Tags").is_none(),
        "a create answer has no Tags (recorded)"
    );
    w.ok(
        "AddRoleToInstanceProfile",
        &json!({"InstanceProfileName": "p", "RoleName": "r"}),
    );
    let got = w.ok("GetInstanceProfile", &json!({"InstanceProfileName": "p"}));
    assert_eq!(got["InstanceProfile"]["Roles"][0]["RoleName"], "r");
    assert_eq!(got["InstanceProfile"]["Tags"], json!([]));
    for role in ["r", "r2"] {
        let (code, message) = w.err(
            "AddRoleToInstanceProfile",
            &json!({"InstanceProfileName": "p", "RoleName": role}),
        );
        assert_eq!(
            (code.as_str(), message.as_str()),
            (
                "LimitExceededException",
                "Cannot exceed quota for InstanceSessionsPerInstanceProfile: 1"
            )
        );
    }
    assert_eq!(
        w.err("DeleteRole", &json!({"RoleName": "r"})).0,
        "DeleteConflictException"
    );
    assert_eq!(
        w.ok("ListInstanceProfilesForRole", &json!({"RoleName": "r"}))["InstanceProfiles"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn account_settings_providers_and_service_linked_roles() {
    let mut w = World::new();
    w.ok("CreateAccountAlias", &json!({"AccountAlias": "acme"}));
    assert_eq!(
        w.ok("ListAccountAliases", &json!({}))["AccountAliases"],
        json!(["acme"])
    );
    assert_eq!(
        w.err("GetAccountPasswordPolicy", &json!({})).1,
        "The Password Policy with domain name 000000000000 cannot be found."
    );
    w.ok(
        "UpdateAccountPasswordPolicy",
        &json!({"MinimumPasswordLength": 12, "RequireSymbols": true}),
    );
    let p = w.ok("GetAccountPasswordPolicy", &json!({}))["PasswordPolicy"].clone();
    assert_eq!(
        (
            p["MinimumPasswordLength"].as_i64(),
            p["RequireSymbols"].clone()
        ),
        (Some(12), json!(true))
    );
    let oidc = w.ok("CreateOpenIDConnectProvider", &json!({"Url": "https://token.example.com", "ClientIDList": ["a"], "ThumbprintList": ["0123456789012345678901234567890123456789"]}));
    let arn = oidc["OpenIDConnectProviderArn"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        arn,
        "arn:aws:iam::000000000000:oidc-provider/token.example.com"
    );
    w.ok(
        "AddClientIDToOpenIDConnectProvider",
        &json!({"OpenIDConnectProviderArn": arn, "ClientID": "b"}),
    );
    assert_eq!(
        w.ok(
            "GetOpenIDConnectProvider",
            &json!({"OpenIDConnectProviderArn": arn})
        )["ClientIDList"],
        json!(["a", "b"])
    );
    let slr = w.ok(
        "CreateServiceLinkedRole",
        &json!({"AWSServiceName": "elasticbeanstalk.amazonaws.com"}),
    );
    assert_eq!(slr["Role"]["RoleName"], "AWSServiceRoleForElasticbeanstalk");
    assert_eq!(
        slr["Role"]["Path"],
        "/aws-service-role/elasticbeanstalk.amazonaws.com/"
    );
    w.ok(
        "DeleteServiceLinkedRole",
        &json!({"RoleName": "AWSServiceRoleForElasticbeanstalk"}),
    );
    assert_eq!(
        w.ok("GetAccountSummary", &json!({}))["SummaryMap"]["Roles"],
        0
    );
}
