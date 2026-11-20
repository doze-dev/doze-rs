//! SSH keys, service-specific credentials, console passwords, and what a user's credentials do to its rename and delete.

use serde_json::json;

use super::World;

const SSH_RSA: &str = include_str!("testdata/rsa.pub");
const PEM_PUBLIC: &str = include_str!("testdata/rsa-pub.pem");
const SSH_SMALL: &str = include_str!("testdata/small.pub");
const SSH_ED25519: &str = include_str!("testdata/ed25519.pub");

#[test]
fn an_ssh_key_is_rsa_of_2048_bits_and_comes_back_in_either_encoding() {
    let mut w = World::new();
    w.ok("CreateUser", &json!({"UserName": "u"}));
    let made = w.ok(
        "UploadSSHPublicKey",
        &json!({"UserName": "u", "SSHPublicKeyBody": SSH_RSA}),
    )["SSHPublicKey"]
        .clone();
    // What `ssh-keygen -l -E md5` says of the key.
    assert_eq!(
        made["Fingerprint"],
        "cf:f7:6d:3a:91:7c:37:dc:f9:4e:ea:3b:de:49:61:41"
    );
    let id = made["SSHPublicKeyId"].as_str().unwrap().to_owned();
    assert!(id.starts_with("APKA"));
    let pem = w.ok(
        "GetSSHPublicKey",
        &json!({"UserName": "u", "SSHPublicKeyId": id, "Encoding": "PEM"}),
    )["SSHPublicKey"]["SSHPublicKeyBody"]
        .clone();
    assert_eq!(pem.as_str().unwrap().trim(), PEM_PUBLIC.trim());
    let ssh = w.ok(
        "GetSSHPublicKey",
        &json!({"UserName": "u", "SSHPublicKeyId": id, "Encoding": "SSH"}),
    )["SSHPublicKey"]["SSHPublicKeyBody"]
        .clone();
    assert_eq!(ssh.as_str().unwrap().trim(), SSH_RSA.trim());
}

#[test]
fn an_ssh_key_must_be_new_and_rsa_of_2048_bits() {
    let mut w = World::new();
    w.ok("CreateUser", &json!({"UserName": "u"}));
    w.ok(
        "UploadSSHPublicKey",
        &json!({"UserName": "u", "SSHPublicKeyBody": SSH_RSA}),
    );
    assert_eq!(
        w.err(
            "UploadSSHPublicKey",
            &json!({"UserName": "u", "SSHPublicKeyBody": PEM_PUBLIC})
        )
        .0,
        "DuplicateSSHPublicKeyException",
        "the same key in PEM"
    );
    assert_eq!(
        w.err(
            "UploadSSHPublicKey",
            &json!({"UserName": "u", "SSHPublicKeyBody": SSH_SMALL})
        )
        .0,
        "InvalidPublicKeyException"
    );
    assert_eq!(
        w.err(
            "UploadSSHPublicKey",
            &json!({"UserName": "u", "SSHPublicKeyBody": SSH_ED25519})
        )
        .0,
        "UnrecognizedPublicKeyEncodingException"
    );
}

#[test]
fn an_ssh_key_is_deactivated_holds_its_user_and_is_deleted() {
    let mut w = World::new();
    w.ok("CreateUser", &json!({"UserName": "u"}));
    let id = w.ok(
        "UploadSSHPublicKey",
        &json!({"UserName": "u", "SSHPublicKeyBody": SSH_RSA}),
    )["SSHPublicKey"]["SSHPublicKeyId"]
        .clone();
    w.ok(
        "UpdateSSHPublicKey",
        &json!({"UserName": "u", "SSHPublicKeyId": id, "Status": "Inactive"}),
    );
    assert_eq!(
        w.ok("ListSSHPublicKeys", &json!({"UserName": "u"}))["SSHPublicKeys"][0]["Status"],
        "Inactive"
    );
    assert_eq!(
        w.err("DeleteUser", &json!({"UserName": "u"})).1,
        "Cannot delete entity, must remove referenced objects first."
    );
    w.ok(
        "DeleteSSHPublicKey",
        &json!({"UserName": "u", "SSHPublicKeyId": id}),
    );
    assert_eq!(
        w.err(
            "GetSSHPublicKey",
            &json!({"UserName": "u", "SSHPublicKeyId": id, "Encoding": "SSH"})
        )
        .0,
        "NoSuchEntityException"
    );
    w.ok("DeleteUser", &json!({"UserName": "u"}));
}

#[test]
fn service_specific_credentials_show_their_secret_on_create_and_reset_only() {
    let mut w = World::new();
    w.ok("CreateUser", &json!({"UserName": "jane"}));
    let made = w.ok(
        "CreateServiceSpecificCredential",
        &json!({"UserName": "jane", "ServiceName": "codecommit.amazonaws.com"}),
    )["ServiceSpecificCredential"]
        .clone();
    assert_eq!(made["ServiceUserName"], "jane-at-000000000000");
    let password = made["ServicePassword"].as_str().unwrap().to_owned();
    let id = made["ServiceSpecificCredentialId"]
        .as_str()
        .unwrap()
        .to_owned();
    let codecommit = json!({"UserName": "jane", "ServiceName": "codecommit.amazonaws.com"});
    w.ok("CreateServiceSpecificCredential", &codecommit);
    let refused = w.err("CreateServiceSpecificCredential", &codecommit).0;
    assert_eq!(refused, "LimitExceededException");
    let sqs = json!({"UserName": "jane", "ServiceName": "sqs.amazonaws.com"});
    let refused = w.err("CreateServiceSpecificCredential", &sqs).0;
    assert_eq!(refused, "NoSuchEntityException");
    w.ok(
        "CreateServiceSpecificCredential",
        &json!({"UserName": "jane", "ServiceName": "bedrock.amazonaws.com"}),
    );
    let listed = w.ok(
        "ListServiceSpecificCredentials",
        &json!({"UserName": "jane", "ServiceName": "codecommit.amazonaws.com"}),
    )["ServiceSpecificCredentials"]
        .clone();
    assert_eq!(listed.as_array().unwrap().len(), 2);
    assert!(listed[0].get("ServicePassword").is_none());
    let everyone = w.ok("ListServiceSpecificCredentials", &json!({"AllUsers": true}));
    assert_eq!(
        everyone["ServiceSpecificCredentials"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(everyone["IsTruncated"], false);
    let reset = w.ok(
        "ResetServiceSpecificCredential",
        &json!({"UserName": "jane", "ServiceSpecificCredentialId": id}),
    )["ServiceSpecificCredential"]
        .clone();
    assert_ne!(reset["ServicePassword"].as_str().unwrap(), password);
    assert_eq!(reset["ServiceUserName"], "jane-at-000000000000");
    w.ok(
        "UpdateServiceSpecificCredential",
        &json!({"UserName": "jane", "ServiceSpecificCredentialId": id, "Status": "Inactive"}),
    );
    assert_eq!(
        w.err("DeleteUser", &json!({"UserName": "jane"})).1,
        "Cannot delete entity, must remove referenced objects first."
    );
    w.ok(
        "DeleteServiceSpecificCredential",
        &json!({"UserName": "jane", "ServiceSpecificCredentialId": id}),
    );
    assert_eq!(
        w.err(
            "DeleteServiceSpecificCredential",
            &json!({"UserName": "jane", "ServiceSpecificCredentialId": id})
        )
        .0,
        "NoSuchEntityException"
    );
}

#[test]
fn an_api_key_has_an_alias_and_a_secret_and_may_expire() {
    let mut w = World::new();
    w.ok("CreateUser", &json!({"UserName": "jane"}));
    let key = w.ok(
        "CreateServiceSpecificCredential",
        &json!({"UserName": "jane", "ServiceName": "bedrock.amazonaws.com", "CredentialAgeDays": 30}),
    )["ServiceSpecificCredential"]
        .clone();
    assert!(
        key["ServiceCredentialSecret"].is_string() && key["ServiceCredentialAlias"].is_string()
    );
    assert!(key.get("ServicePassword").is_none());
    assert_eq!(
        key["ExpirationDate"].as_f64(),
        Some(1_700_000_000.0 + 30.0 * 86_400.0)
    );
}

fn with_policy() -> World {
    let mut w = World::new();
    w.ok("CreateUser", &json!({"UserName": "u"}));
    w.ok(
        "UpdateAccountPasswordPolicy",
        &json!({"MinimumPasswordLength": 10, "RequireNumbers": true, "PasswordReusePrevention": 2}),
    );
    w
}

#[test]
fn a_password_is_held_to_the_policy() {
    let mut w = with_policy();
    assert_eq!(
        w.err(
            "CreateLoginProfile",
            &json!({"UserName": "u", "Password": "short1"})
        ),
        (
            "PasswordPolicyViolationException".into(),
            "Password should have a minimum length of 10".into()
        )
    );
    assert_eq!(
        w.err(
            "CreateLoginProfile",
            &json!({"UserName": "u", "Password": "longenough"})
        )
        .1,
        "Password should have at least one number"
    );
}

#[test]
fn a_user_changes_its_own_password_under_the_policy() {
    let mut w = with_policy();
    w.ok(
        "CreateLoginProfile",
        &json!({"UserName": "u", "Password": "longenough1", "PasswordResetRequired": true}),
    );
    assert_eq!(
        w.ok("GetLoginProfile", &json!({"UserName": "u"}))["LoginProfile"]["PasswordResetRequired"],
        true
    );
    // The root is not an IAM user.
    assert_eq!(
        w.err(
            "ChangePassword",
            &json!({"OldPassword": "longenough1", "NewPassword": "another1234"})
        )
        .0,
        "InvalidUserTypeException"
    );
    let key = w.ok("CreateAccessKey", &json!({"UserName": "u"}))["AccessKey"]["AccessKeyId"]
        .as_str()
        .unwrap()
        .to_owned();
    w.iam.access_key_id = key;
    assert_eq!(
        w.err(
            "ChangePassword",
            &json!({"OldPassword": "wrong", "NewPassword": "another1234"})
        )
        .0,
        "AccessDenied"
    );
    assert_eq!(
        w.err(
            "ChangePassword",
            &json!({"OldPassword": "longenough1", "NewPassword": "longenough1"})
        )
        .0,
        "PasswordPolicyViolationException",
        "used too recently"
    );
    w.ok(
        "ChangePassword",
        &json!({"OldPassword": "longenough1", "NewPassword": "another1234"}),
    );
    assert_eq!(
        w.ok("GetLoginProfile", &json!({"UserName": "u"}))["LoginProfile"]["PasswordResetRequired"],
        false
    );
    w.ok(
        "ChangePassword",
        &json!({"OldPassword": "another1234", "NewPassword": "third12345"}),
    );
    w.iam.access_key_id = "test".into();
    assert_eq!(
        w.err("DeleteUser", &json!({"UserName": "u"})).1,
        "Cannot delete entity, must delete access keys first."
    );
}

#[test]
fn a_renamed_user_keeps_its_credentials_and_a_login_profile_holds_it() {
    let mut w = World::new();
    w.ok("CreateUser", &json!({"UserName": "u"}));
    w.ok(
        "CreateLoginProfile",
        &json!({"UserName": "u", "Password": "pw"}),
    );
    w.ok(
        "UploadSSHPublicKey",
        &json!({"UserName": "u", "SSHPublicKeyBody": SSH_RSA}),
    );
    w.ok("UpdateUser", &json!({"UserName": "u", "NewUserName": "v"}));
    assert_eq!(
        w.ok("ListSSHPublicKeys", &json!({"UserName": "v"}))["SSHPublicKeys"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(w.core.held.passwords.get("v").is_some());
    assert_eq!(
        w.err("DeleteUser", &json!({"UserName": "v"})).1,
        "Cannot delete entity, must delete login profile first."
    );
}

#[test]
fn the_sts_preference_is_kept_and_reported() {
    let mut w = World::new();
    assert_eq!(
        w.ok("GetAccountSummary", &json!({}))["SummaryMap"]["GlobalEndpointTokenVersion"],
        1
    );
    w.ok(
        "SetSecurityTokenServicePreferences",
        &json!({"GlobalEndpointTokenVersion": "v2Token"}),
    );
    assert_eq!(
        w.ok("GetAccountSummary", &json!({}))["SummaryMap"]["GlobalEndpointTokenVersion"],
        2
    );
}
