//! Virtual MFA devices: made, enabled with two real TOTP codes, listed, spent by STS, and in the way of deleting their user.

use aws_peers::{Mode, Need};
use aws_wire::service::Caller;
use serde_json::{Value, json};

use super::{START, TRUST, World};
use crate::totp;

const SERIAL: &str = "arn:aws:iam::000000000000:mfa/phone";

/// The codes the device `serial` shows at the clock's step and the next.
fn codes(w: &World, serial: &str, offset: i64) -> (String, String) {
    let seed = &w.core.held.mfa.get(serial).unwrap().seed;
    let step = totp::step_of(w.core.now_ms()) + offset;
    (
        totp::code(seed, step).unwrap(),
        totp::code(seed, step + 1).unwrap(),
    )
}

fn now_code(w: &World, serial: &str) -> String {
    codes(w, serial, 0).0
}

/// A user `u` with a key, and its enabled device `phone`; the key's id.
fn user_with_device(w: &mut World) -> String {
    w.ok("CreateUser", &json!({"UserName": "u"}));
    w.ok(
        "CreateVirtualMFADevice",
        &json!({"VirtualMFADeviceName": "phone"}),
    );
    let (a, b) = codes(w, SERIAL, -1);
    w.ok(
        "EnableMFADevice",
        &json!({"UserName": "u", "SerialNumber": SERIAL, "AuthenticationCode1": a, "AuthenticationCode2": b}),
    );
    w.ok("CreateAccessKey", &json!({"UserName": "u"}))["AccessKey"]["AccessKeyId"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn a_device_shows_its_seed_once_and_is_enabled_by_two_consecutive_codes() {
    let mut w = World::new();
    let made = w.ok(
        "CreateVirtualMFADevice",
        &json!({"VirtualMFADeviceName": "phone", "Tags": [{"Key": "team", "Value": "a"}]}),
    )["VirtualMFADevice"]
        .clone();
    assert_eq!(made["SerialNumber"], SERIAL);
    // The blob is the base32 text of the seed.
    let text = aws_core::base64::decode(made["Base32StringSeed"].as_str().unwrap()).unwrap();
    let seed = &w.core.held.mfa.get(SERIAL).unwrap().seed;
    assert_eq!(String::from_utf8(text).unwrap(), totp::base32(seed));
    assert_eq!(seed.len(), 40);
    assert_eq!(made["Tags"], json!([{"Key": "team", "Value": "a"}]));
    assert_eq!(
        w.err(
            "CreateVirtualMFADevice",
            &json!({"VirtualMFADeviceName": "phone"})
        )
        .0,
        "EntityAlreadyExistsException"
    );
    w.ok("CreateUser", &json!({"UserName": "u"}));
    let (a, b) = codes(&w, SERIAL, 0);
    let enable = json!({"UserName": "u", "SerialNumber": SERIAL, "AuthenticationCode1": b, "AuthenticationCode2": a});
    assert_eq!(
        w.err("EnableMFADevice", &enable).0,
        "InvalidAuthenticationCodeException",
        "out of order"
    );
    w.ok(
        "EnableMFADevice",
        &json!({"UserName": "u", "SerialNumber": SERIAL, "AuthenticationCode1": a, "AuthenticationCode2": b}),
    );
    let listed = w.ok("ListMFADevices", &json!({"UserName": "u"}));
    assert_eq!(listed["MFADevices"][0]["SerialNumber"], SERIAL);
    assert_eq!(listed["MFADevices"][0]["UserName"], "u");
    let virtual_list = w.ok(
        "ListVirtualMFADevices",
        &json!({"AssignmentStatus": "Assigned"}),
    );
    let device = &virtual_list["VirtualMFADevices"][0];
    assert_eq!(device["User"]["UserName"], "u");
    assert!(device.get("Base32StringSeed").is_none(), "shown once");
    assert_eq!(
        w.ok(
            "ListVirtualMFADevices",
            &json!({"AssignmentStatus": "Unassigned"})
        )["VirtualMFADevices"],
        json!([])
    );
    let summary = w.ok("GetAccountSummary", &json!({}))["SummaryMap"].clone();
    assert_eq!(
        (
            summary["MFADevices"].as_i64(),
            summary["MFADevicesInUse"].as_i64()
        ),
        (Some(1), Some(1))
    );
}

#[test]
fn an_enabled_device_holds_its_user_and_must_be_deactivated_before_it_goes() {
    let mut w = World::new();
    let key = user_with_device(&mut w);
    w.ok(
        "DeleteAccessKey",
        &json!({"UserName": "u", "AccessKeyId": key}),
    );
    assert_eq!(
        w.err("DeleteUser", &json!({"UserName": "u"})),
        (
            "DeleteConflictException".into(),
            "Cannot delete entity, must remove tokens from principal first.".into()
        )
    );
    assert_eq!(
        w.err("DeleteVirtualMFADevice", &json!({"SerialNumber": SERIAL})),
        (
            "DeleteConflictException".into(),
            "MFA VirtualDevice in use. Must deactivate first.".into()
        )
    );
    w.ok(
        "DeactivateMFADevice",
        &json!({"UserName": "u", "SerialNumber": SERIAL}),
    );
    assert_eq!(
        w.err(
            "DeactivateMFADevice",
            &json!({"UserName": "u", "SerialNumber": SERIAL})
        ),
        (
            "NoSuchEntityException".into(),
            "MFA Device invalid for user.".into()
        )
    );
    w.ok("DeleteVirtualMFADevice", &json!({"SerialNumber": SERIAL}));
    w.ok("DeleteUser", &json!({"UserName": "u"}));
    assert_eq!(
        w.err("DeleteVirtualMFADevice", &json!({"SerialNumber": SERIAL}))
            .0,
        "NoSuchEntityException"
    );
}

#[test]
fn a_device_is_resynced_with_two_codes_and_tagged() {
    let mut w = World::new();
    user_with_device(&mut w);
    // Twenty minutes off: two consecutive codes from then bring it back.
    let (a, b) = codes(&w, SERIAL, 40);
    w.ok(
        "ResyncMFADevice",
        &json!({"UserName": "u", "SerialNumber": SERIAL, "AuthenticationCode1": a, "AuthenticationCode2": b}),
    );
    let (a, b) = codes(&w, SERIAL, 500);
    assert_eq!(
        w.err(
            "ResyncMFADevice",
            &json!({"UserName": "u", "SerialNumber": SERIAL, "AuthenticationCode1": a, "AuthenticationCode2": b})
        )
        .0,
        "InvalidAuthenticationCodeException"
    );
    w.ok(
        "TagMFADevice",
        &json!({"SerialNumber": SERIAL, "Tags": [{"Key": "b", "Value": "2"}, {"Key": "a", "Value": "1"}]}),
    );
    w.ok(
        "UntagMFADevice",
        &json!({"SerialNumber": SERIAL, "TagKeys": ["b"]}),
    );
    assert_eq!(
        w.ok("ListMFADeviceTags", &json!({"SerialNumber": SERIAL}))["Tags"],
        json!([{"Key": "a", "Value": "1"}])
    );
    // GetMFADevice answers for FIDO keys only, as the model says, and refuses a virtual device's serial as AWS does.
    assert_eq!(
        w.err(
            "GetMFADevice",
            &json!({"SerialNumber": SERIAL, "UserName": "u"})
        )
        .0,
        "ValidationError"
    );
}

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

/// A policy that allows `sqs:*` only with MFA.
const WITH_MFA: &str = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"sqs:*","Resource":"*","Condition":{"Bool":{"aws:MultiFactorAuthPresent":"true"}}}]}"#;

#[test]
fn a_session_token_made_with_a_good_code_carries_mfa_and_a_code_is_good_once() {
    let mut w = World::new();
    let key = user_with_device(&mut w);
    w.ok(
        "PutUserPolicy",
        &json!({"UserName": "u", "PolicyName": "p", "PolicyDocument": WITH_MFA}),
    );
    w.sts.access_key_id.clone_from(&key);
    // A wrong code, and a device that is not the caller's, are refused.
    let (code, _) = w.sts_err(
        "GetSessionToken",
        &json!({"SerialNumber": SERIAL, "TokenCode": "000000"}),
    );
    assert_eq!(code, "AccessDenied");
    let (code, message) = w.sts_err(
        "GetSessionToken",
        &json!({"SerialNumber": "arn:aws:iam::000000000000:mfa/other", "TokenCode": "123456"}),
    );
    assert_eq!(code, "AccessDenied");
    assert!(message.contains("unable to validate MFA code"), "{message}");
    // The enabling codes were spent: the next step's code is the first good one.
    w.clock.advance(30_000);
    let good = now_code(&w, SERIAL);
    let with = w.sts_ok(
        "GetSessionToken",
        &json!({"SerialNumber": SERIAL, "TokenCode": good}),
    )["Credentials"]
        .clone();
    assert_eq!(
        w.sts_err(
            "GetSessionToken",
            &json!({"SerialNumber": SERIAL, "TokenCode": good})
        )
        .0,
        "AccessDenied",
        "a code is good once"
    );
    let without = w.sts_ok("GetSessionToken", &json!({}))["Credentials"].clone();

    w.core.mode = Mode::Enforce;
    let q = [Need::new("sqs:SendMessage", "*")];
    let session = |c: &Value| {
        caller(
            c["AccessKeyId"].as_str().unwrap(),
            c["SessionToken"].as_str(),
        )
    };
    assert!(w.core.authorize(&session(&with), &q).is_ok());
    assert!(w.core.authorize(&session(&without), &q).is_err());
    assert!(
        w.core.authorize(&caller(&key, None), &q).is_err(),
        "a long-term key has no MFA"
    );
}

#[test]
fn a_trust_policy_that_asks_for_mfa_is_met_by_a_code() {
    let mut w = World::new();
    let key = user_with_device(&mut w);
    let trust = TRUST.replace(
        r#"{"Service":"lambda.amazonaws.com"}"#,
        r#"{"AWS":"arn:aws:iam::000000000000:user/u"}"#,
    );
    let trust = trust.replace(
        r#""Action":"sts:AssumeRole""#,
        r#""Action":"sts:AssumeRole","Condition":{"Bool":{"aws:MultiFactorAuthPresent":"true"}}"#,
    );
    w.ok(
        "CreateRole",
        &json!({"RoleName": "admin", "AssumeRolePolicyDocument": trust}),
    );
    w.ok(
        "PutUserPolicy",
        &json!({"UserName": "u", "PolicyName": "p", "PolicyDocument": r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"sts:AssumeRole","Resource":"*"}]}"#}),
    );
    w.core.mode = Mode::Enforce;
    w.sts.access_key_id = key;
    let arn = "arn:aws:iam::000000000000:role/admin";
    let (code, _) = w.sts_err(
        "AssumeRole",
        &json!({"RoleArn": arn, "RoleSessionName": "ss"}),
    );
    assert_eq!(code, "AccessDenied");
    w.clock.set(START + 3600 * 1000);
    let good = now_code(&w, SERIAL);
    let got = w.sts_ok(
        "AssumeRole",
        &json!({"RoleArn": arn, "RoleSessionName": "ss", "SerialNumber": SERIAL, "TokenCode": good}),
    );
    assert!(got["Credentials"]["SessionToken"].is_string());
}
