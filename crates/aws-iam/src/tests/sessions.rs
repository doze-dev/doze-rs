//! STS: who a request is, and the sessions roles give out.

use serde_json::json;

use super::{TRUST, World};
use crate::session::Session;

#[test]
fn an_unknown_key_is_the_account_root() {
    let mut w = World::new();
    let who = w.sts_ok("GetCallerIdentity", &json!({}));
    assert_eq!(
        (
            who["Account"].as_str(),
            who["Arn"].as_str(),
            who["UserId"].as_str()
        ),
        (
            Some("000000000000"),
            Some("arn:aws:iam::000000000000:root"),
            Some("000000000000")
        )
    );
}

#[test]
fn an_iam_users_key_is_that_user() {
    let mut w = World::new();
    let user = w.ok("CreateUser", &json!({"UserName": "u"}))["User"].clone();
    let key = w.ok("CreateAccessKey", &json!({"UserName": "u"}))["AccessKey"]["AccessKeyId"]
        .as_str()
        .unwrap()
        .to_owned();
    w.sts.access_key_id = key;
    let who = w.sts_ok("GetCallerIdentity", &json!({}));
    assert_eq!(
        (who["Arn"].clone(), who["UserId"].clone()),
        (user["Arn"].clone(), user["UserId"].clone())
    );
    // With no UserName, the keys and the user are the caller's.
    w.iam.access_key_id = w.sts.access_key_id.clone();
    assert_eq!(w.ok("GetUser", &json!({}))["User"]["UserName"], "u");
}

#[test]
fn an_assumed_role_is_a_session_that_the_account_recognises_and_that_runs_out() {
    let mut w = World::new();
    let role = w.ok(
        "CreateRole",
        &json!({"RoleName": "r", "AssumeRolePolicyDocument": TRUST, "MaxSessionDuration": 7200}),
    )["Role"]
        .clone();
    let got = w.sts_ok(
        "AssumeRole",
        &json!({"RoleArn": role["Arn"], "RoleSessionName": "sess", "DurationSeconds": 1800}),
    );
    let creds = &got["Credentials"];
    assert!(creds["AccessKeyId"].as_str().unwrap().starts_with("ASIA"));
    assert_eq!(
        got["AssumedRoleUser"]["Arn"],
        "arn:aws:sts::000000000000:assumed-role/r/sess"
    );
    assert_eq!(
        got["AssumedRoleUser"]["AssumedRoleId"],
        format!("{}:sess", role["RoleId"].as_str().unwrap())
    );
    // Carrying the token, a call is that session.
    w.sts.session_token = creds["SessionToken"].as_str().map(str::to_owned);
    let who = w.sts_ok("GetCallerIdentity", &json!({}));
    assert_eq!(who["Arn"], "arn:aws:sts::000000000000:assumed-role/r/sess");
    // The token is not recognised by another account's key, nor after its time.
    let key = w.core.account().signing_key;
    assert!(Session::verify(creds["SessionToken"].as_str().unwrap(), &key, super::START).is_ok());
    w.clock.set(super::START + 1801 * 1000);
    let who = w.sts_ok("GetCallerIdentity", &json!({}));
    assert_eq!(
        who["Arn"], "arn:aws:iam::000000000000:root",
        "an expired token is not that session"
    );
}

#[test]
fn durations_and_unknown_roles() {
    let mut w = World::new();
    let arn = "arn:aws:iam::000000000000:role/r";
    // A role that does not exist can be assumed in soft mode (CDK's bootstrap roles are never made locally).
    assert!(
        w.sts_ok(
            "AssumeRole",
            &json!({"RoleArn": arn, "RoleSessionName": "sess"})
        )["Credentials"]["SessionToken"]
            .is_string()
    );
    w.ok(
        "CreateRole",
        &json!({"RoleName": "r", "AssumeRolePolicyDocument": TRUST}),
    );
    assert_eq!(
        w.sts_err(
            "AssumeRole",
            &json!({"RoleArn": arn, "RoleSessionName": "sess", "DurationSeconds": 7200})
        )
        .0,
        "ValidationError"
    );
    assert_eq!(
        w.sts_err(
            "AssumeRole",
            &json!({"RoleArn": arn, "RoleSessionName": "sess", "DurationSeconds": 100})
        )
        .0,
        "ValidationError"
    );
    assert_eq!(
        w.sts_err(
            "AssumeRole",
            &json!({"RoleArn": "not an arn", "RoleSessionName": "sess"})
        )
        .0,
        "ValidationError"
    );
}

#[test]
fn session_tokens_federation_and_web_identity() {
    let mut w = World::new();
    let t = w.sts_ok("GetSessionToken", &json!({"DurationSeconds": 900}));
    assert!(
        t["Credentials"]["AccessKeyId"]
            .as_str()
            .unwrap()
            .starts_with("ASIA")
    );
    assert_eq!(
        w.sts_err("GetSessionToken", &json!({"DurationSeconds": 10}))
            .0,
        "ValidationError"
    );
    let f = w.sts_ok("GetFederationToken", &json!({"Name": "bob"}));
    assert_eq!(
        f["FederatedUser"]["Arn"],
        "arn:aws:sts::000000000000:federated-user/bob"
    );
    w.sts.session_token = f["Credentials"]["SessionToken"].as_str().map(str::to_owned);
    assert_eq!(
        w.sts_ok("GetCallerIdentity", &json!({}))["Arn"],
        "arn:aws:sts::000000000000:federated-user/bob"
    );
    w.sts.session_token = None;
    // A web identity token is read for its claims and its signature is not checked.
    let claims =
        aws_core::base64::encode(br#"{"sub":"alice","iss":"https://idp.example","aud":"app"}"#);
    let jwt = format!(
        "e30.{}.sig",
        claims
            .replace('+', "-")
            .replace('/', "_")
            .trim_end_matches('=')
    );
    let web = w.sts_ok("AssumeRoleWithWebIdentity", &json!({"RoleArn": "arn:aws:iam::000000000000:role/web", "RoleSessionName": "web", "WebIdentityToken": jwt}));
    assert_eq!(
        (
            web["SubjectFromWebIdentityToken"].as_str(),
            web["Audience"].as_str()
        ),
        (Some("alice"), Some("app"))
    );
    assert_eq!(
        w.sts_ok(
            "GetAccessKeyInfo",
            &json!({"AccessKeyId": "AKIAEXAMPLE0000000000"})
        )["Account"],
        "000000000000"
    );
}

#[test]
fn a_saml_assertion_is_read_for_its_subject_issuer_and_session_name_and_echoed() {
    let mut w = World::new();
    let assertion = aws_core::base64::encode(crate::saml::tests::ASSERTION.as_bytes());
    let out = w.sts_ok(
        "AssumeRoleWithSAML",
        &json!({
            "RoleArn": "arn:aws:iam::000000000000:role/r",
            "PrincipalArn": "arn:aws:iam::000000000000:saml-provider/idp",
            "SAMLAssertion": assertion,
        }),
    );
    assert_eq!(out["Subject"], "alice&co");
    assert_eq!(out["SubjectType"], "persistent");
    assert_eq!(out["Issuer"], "https://idp.example.com/saml");
    assert_eq!(out["Audience"], "https://signin.aws.amazon.com/saml");
    assert_eq!(
        out["AssumedRoleUser"]["Arn"],
        "arn:aws:sts::000000000000:assumed-role/r/alice@example.com"
    );
    // BASE64(SHA1(issuer + account + "/" + provider)), worked out by hand for this assertion.
    let expected = {
        use sha1::Digest;
        aws_core::base64::encode(&sha1::Sha1::digest(
            b"https://idp.example.com/saml000000000000/idp",
        ))
    };
    assert_eq!(out["NameQualifier"], expected.as_str());
    let token = out["Credentials"]["SessionToken"].as_str().unwrap();
    w.sts.session_token = Some(token.to_owned());
    assert_eq!(
        w.sts_ok("GetCallerIdentity", &json!({}))["Arn"],
        "arn:aws:sts::000000000000:assumed-role/r/alice@example.com"
    );
    w.sts.session_token = None;
    // What is not an assertion is refused.
    let (code, _) = w.sts_err(
        "AssumeRoleWithSAML",
        &json!({
            "RoleArn": "arn:aws:iam::000000000000:role/r",
            "PrincipalArn": "arn:aws:iam::000000000000:saml-provider/idp",
            "SAMLAssertion": aws_core::base64::encode(b"<not-saml"),
        }),
    );
    assert_eq!(code, "InvalidIdentityTokenException");
}
