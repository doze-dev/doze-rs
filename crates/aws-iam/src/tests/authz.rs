//! Authorization: who the caller is, and what the policies say.

use aws_peers::{Mode, Need};
use aws_wire::service::Caller;
use serde_json::json;

use super::{TRUST, World};

fn caller(key: &str, token: Option<&str>) -> Caller {
    Caller {
        access_key_id: key.into(),
        account_id: "000000000000".into(),
        region: "us-east-1".into(),
        endpoint: "http://localhost:4566".into(),
        service: None,
        session_token: token.map(str::to_owned),
    }
}

const QUEUE: &str = "arn:aws:sqs:us-east-1:000000000000:jobs";
const SEND: &str = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"sqs:SendMessage","Resource":"arn:aws:sqs:*:*:jobs"}]}"#;

fn user_with_key(w: &mut World, name: &str) -> String {
    w.ok("CreateUser", &json!({"UserName": name}));
    w.ok("CreateAccessKey", &json!({"UserName": name}))["AccessKey"]["AccessKeyId"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn refusal(w: &World, c: &Caller, action: &str) -> Option<String> {
    w.core
        .authorize(c, &[Need::new(action, QUEUE)])
        .err()
        .map(|e| e.message)
}

#[test]
fn the_root_and_keys_iam_did_not_issue_are_allowed_everything() {
    let mut w = World::new();
    w.core.mode = Mode::Enforce;
    assert_eq!(refusal(&w, &caller("test", None), "sqs:DeleteQueue"), None);
    // A key that looks issued but is not known is not an identity either.
    assert_eq!(
        refusal(&w, &caller("AKIAUNKNOWN", None), "sqs:DeleteQueue"),
        None
    );
}

#[test]
fn a_user_may_do_what_its_policies_allow_and_is_told_why_not() {
    let mut w = World::new();
    w.core.mode = Mode::Enforce;
    let key = user_with_key(&mut w, "alice");
    let c = caller(&key, None);
    let arn = "arn:aws:iam::000000000000:user/alice";
    assert_eq!(
        refusal(&w, &c, "sqs:SendMessage").unwrap(),
        format!(
            "User: {arn} is not authorized to perform: sqs:SendMessage on resource: {QUEUE} because no identity-based policy allows the sqs:SendMessage action"
        )
    );
    w.ok(
        "PutUserPolicy",
        &json!({"UserName": "alice", "PolicyName": "send", "PolicyDocument": SEND}),
    );
    assert_eq!(refusal(&w, &c, "sqs:SendMessage"), None);
    assert!(refusal(&w, &c, "sqs:DeleteQueue").is_some());
    // A deny wins, and is named.
    let deny = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Deny","Action":"sqs:*","Resource":"*"}]}"#;
    w.ok(
        "PutUserPolicy",
        &json!({"UserName": "alice", "PolicyName": "deny", "PolicyDocument": deny}),
    );
    assert!(
        refusal(&w, &c, "sqs:SendMessage")
            .unwrap()
            .ends_with("with an explicit deny in an identity-based policy")
    );
    // Soft mode says nothing to the caller.
    w.core.mode = Mode::Soft;
    assert_eq!(refusal(&w, &c, "sqs:SendMessage"), None);
}

#[test]
fn a_boundary_limits_a_user_and_a_resource_policy_can_name_it_past_the_boundary() {
    let mut w = World::new();
    w.core.mode = Mode::Enforce;
    let key = user_with_key(&mut w, "bob");
    let c = caller(&key, None);
    w.ok(
        "PutUserPolicy",
        &json!({"UserName": "bob", "PolicyName": "send", "PolicyDocument": SEND}),
    );
    let s3 = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"s3:*","Resource":"*"}]}"#;
    let policy = w.ok(
        "CreatePolicy",
        &json!({"PolicyName": "s3", "PolicyDocument": s3}),
    );
    w.ok(
        "PutUserPermissionsBoundary",
        &json!({"UserName": "bob", "PermissionsBoundary": policy["Policy"]["Arn"]}),
    );
    assert!(
        refusal(&w, &c, "sqs:SendMessage")
            .unwrap()
            .ends_with("because no permissions boundary allows the sqs:SendMessage action")
    );
    let named = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"AWS":"arn:aws:iam::000000000000:user/bob"},"Action":"sqs:SendMessage","Resource":"*"}]}"#;
    let need = Need::new("sqs:SendMessage", QUEUE).with_policy(Some(named.to_owned()));
    assert!(w.core.authorize(&c, &[need]).is_ok());
}

#[test]
fn a_role_session_has_its_roles_policies_and_its_session_policy() {
    let mut w = World::new();
    w.core.mode = Mode::Enforce;
    w.ok(
        "CreateRole",
        &json!({"RoleName": "r", "AssumeRolePolicyDocument": TRUST}),
    );
    w.ok(
        "PutRolePolicy",
        &json!({"RoleName": "r", "PolicyName": "send", "PolicyDocument": SEND}),
    );
    let assumed = w.sts_ok(
        "AssumeRole",
        &json!({"RoleArn": "arn:aws:iam::000000000000:role/r", "RoleSessionName": "me"}),
    );
    let (key, token) = (
        assumed["Credentials"]["AccessKeyId"]
            .as_str()
            .unwrap()
            .to_owned(),
        assumed["Credentials"]["SessionToken"]
            .as_str()
            .unwrap()
            .to_owned(),
    );
    let c = caller(&key, Some(&token));
    assert_eq!(refusal(&w, &c, "sqs:SendMessage"), None);
    let denied = refusal(&w, &c, "sqs:DeleteQueue").unwrap();
    assert!(
        denied.starts_with("User: arn:aws:sts::000000000000:assumed-role/r/me is not authorized"),
        "{denied}"
    );
    // A session policy narrows what the role allows.
    let narrow = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"s3:*","Resource":"*"}]}"#;
    let assumed = w.sts_ok("AssumeRole", &json!({"RoleArn": "arn:aws:iam::000000000000:role/r", "RoleSessionName": "me", "Policy": narrow}));
    let c = caller(
        assumed["Credentials"]["AccessKeyId"].as_str().unwrap(),
        assumed["Credentials"]["SessionToken"].as_str(),
    );
    assert!(
        refusal(&w, &c, "sqs:SendMessage")
            .unwrap()
            .ends_with("because no session policy allows the sqs:SendMessage action")
    );
    // A forged token is refused as AWS refuses one.
    let forged = caller(&key, Some("junk.junk"));
    let e = w
        .core
        .authorize(&forged, &[Need::new("sqs:SendMessage", QUEUE)])
        .unwrap_err();
    assert_eq!(e.code, "InvalidClientTokenId");
}

#[test]
fn principal_tags_and_condition_keys_reach_the_policy() {
    let mut w = World::new();
    w.core.mode = Mode::Enforce;
    let key = user_with_key(&mut w, "carol");
    w.ok(
        "TagUser",
        &json!({"UserName": "carol", "Tags": [{"Key": "team", "Value": "blue"}]}),
    );
    let doc = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"sqs:*","Resource":"*","Condition":{"StringEquals":{"aws:PrincipalTag/team":"blue","aws:RequestedRegion":"us-east-1"}}}]}"#;
    w.ok(
        "PutUserPolicy",
        &json!({"UserName": "carol", "PolicyName": "p", "PolicyDocument": doc}),
    );
    assert_eq!(refusal(&w, &caller(&key, None), "sqs:SendMessage"), None);
    w.ok(
        "TagUser",
        &json!({"UserName": "carol", "Tags": [{"Key": "team", "Value": "red"}]}),
    );
    assert!(refusal(&w, &caller(&key, None), "sqs:SendMessage").is_some());
}

fn iam_as(
    w: &mut World,
    key: &str,
    op: &str,
    request: &serde_json::Value,
) -> Result<serde_json::Value, aws_wire::ApiError> {
    let previous = std::mem::replace(&mut w.iam.access_key_id, key.to_owned());
    let out = w.run(op, request);
    w.iam.access_key_id = previous;
    out
}

#[test]
fn iam_guards_itself_and_sts_lets_anyone_see_who_they_are() {
    let mut w = World::new();
    let key = user_with_key(&mut w, "dave");
    w.core.mode = Mode::Enforce;
    let e = iam_as(&mut w, &key, "CreateUser", &json!({"UserName": "eve"})).unwrap_err();
    assert_eq!((e.status, e.code.as_ref()), (403, "AccessDenied"));
    assert!(e.message.contains("is not authorized to perform: iam:CreateUser on resource: arn:aws:iam::000000000000:user/eve"), "{}", e.message);
    // The root is not asked.
    w.ok("CreateUser", &json!({"UserName": "eve"}));
    // Allowed on its own user only.
    let own = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"iam:GetUser","Resource":"arn:aws:iam::000000000000:user/dave"}]}"#;
    w.ok(
        "PutUserPolicy",
        &json!({"UserName": "dave", "PolicyName": "own", "PolicyDocument": own}),
    );
    assert!(iam_as(&mut w, &key, "GetUser", &json!({"UserName": "dave"})).is_ok());
    assert!(iam_as(&mut w, &key, "GetUser", &json!({"UserName": "eve"})).is_err());
    let previous = std::mem::replace(&mut w.sts.access_key_id, key);
    assert_eq!(
        w.sts_ok("GetCallerIdentity", &json!({}))["Arn"],
        "arn:aws:iam::000000000000:user/dave"
    );
    w.sts.access_key_id = previous;
}

#[test]
fn assuming_a_role_needs_the_trust_policy_and_either_the_user_named_or_an_identity_allow() {
    let mut w = World::new();
    let key = user_with_key(&mut w, "frank");
    let direct = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"AWS":"arn:aws:iam::000000000000:user/frank"},"Action":"sts:AssumeRole"}]}"#;
    w.ok(
        "CreateRole",
        &json!({"RoleName": "named", "AssumeRolePolicyDocument": direct}),
    );
    let account = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"AWS":"arn:aws:iam::000000000000:root"},"Action":"sts:AssumeRole"}]}"#;
    w.ok(
        "CreateRole",
        &json!({"RoleName": "open", "AssumeRolePolicyDocument": account}),
    );
    w.ok(
        "CreateRole",
        &json!({"RoleName": "lambda", "AssumeRolePolicyDocument": TRUST}),
    );
    w.core.mode = Mode::Enforce;
    let may = |w: &mut World, role: &str| {
        let previous = std::mem::replace(&mut w.sts.access_key_id, key.clone());
        let out = w.sts_run("AssumeRole", &json!({"RoleArn": format!("arn:aws:iam::000000000000:role/{role}"), "RoleSessionName": "sess"}));
        w.sts.access_key_id = previous;
        out.is_ok()
    };
    assert!(may(&mut w, "named"), "the trust policy names the user");
    assert!(
        !may(&mut w, "open"),
        "the trust policy names the account, so the user needs an allow of its own"
    );
    let allow = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"sts:AssumeRole","Resource":"*"}]}"#;
    w.ok(
        "PutUserPolicy",
        &json!({"UserName": "frank", "PolicyName": "assume", "PolicyDocument": allow}),
    );
    assert!(may(&mut w, "open"));
    assert!(
        !may(&mut w, "lambda"),
        "a trust policy that names someone else lets no one else in"
    );
}
