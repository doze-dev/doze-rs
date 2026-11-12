//! What is refused, in the words AWS gave when the scenarios were recorded (2026-11-09).

use serde_json::json;

use super::World;

#[test]
fn what_aws_refused_it_refuses_in_its_words() {
    let mut w = World::new();
    let absent = "00000000-0000-4000-8000-000000000000";
    let arn = format!("arn:aws:kms:us-east-1:000000000000:key/{absent}");
    assert_eq!(
        w.err("DescribeKey", &json!({"KeyId": absent})),
        (
            "NotFoundException".into(),
            format!("Key '{arn}' does not exist")
        )
    );
    assert_eq!(
        w.err("DescribeKey", &json!({"KeyId": "alias/never"})).1,
        "Alias arn:aws:kms:us-east-1:000000000000:alias/never is not found."
    );
    assert_eq!(
        w.err(
            "Decrypt",
            &json!({"CiphertextBlob": "bm90IGNpcGhlcnRleHQ="})
        ),
        ("InvalidCiphertextException".into(), String::new())
    );
    assert_eq!(
        w.err(
            "CreateKey",
            &json!({"KeySpec": "HMAC_256", "KeyUsage": "ENCRYPT_DECRYPT"})
        ),
        (
            "ValidationException".into(),
            "KeyUsage ENCRYPT_DECRYPT is not compatible with KeySpec HMAC_256".into()
        )
    );
}

#[test]
fn a_key_in_the_wrong_state_or_use_is_refused_in_its_words() {
    let mut w = World::new();
    let id = w.key(&json!({}));
    assert_eq!(
        w.err(
            "ScheduleKeyDeletion",
            &json!({"KeyId": id, "PendingWindowInDays": 3})
        ),
        (
            "ValidationException".into(),
            "PendingWindowInDays must be between 7 and 30".into()
        )
    );
    let arn = format!("arn:aws:kms:us-east-1:000000000000:key/{id}");
    assert_eq!(
        w.err(
            "Sign",
            &json!({"KeyId": id, "Message": "bQ==", "SigningAlgorithm": "ECDSA_SHA_256"})
        )
        .1,
        format!("{arn} key usage is ENCRYPT_DECRYPT which is not valid for Sign.")
    );
    w.ok("DisableKey", &json!({"KeyId": id}));
    assert_eq!(
        w.err("Encrypt", &json!({"KeyId": id, "Plaintext": "eA=="}))
            .1,
        format!("{arn} is disabled.")
    );
    w.ok(
        "ScheduleKeyDeletion",
        &json!({"KeyId": id, "PendingWindowInDays": 7}),
    );
    assert_eq!(
        w.err("Encrypt", &json!({"KeyId": id, "Plaintext": "eA=="}))
            .1,
        format!("{arn} is pending deletion.")
    );

    let signer = w.key(&json!({"KeySpec": "ECC_NIST_P256", "KeyUsage": "SIGN_VERIFY"}));
    let arn = format!("arn:aws:kms:us-east-1:000000000000:key/{signer}");
    assert_eq!(
        w.err("Encrypt", &json!({"KeyId": signer, "Plaintext": "eA=="}))
            .1,
        format!("{arn} key usage is SIGN_VERIFY which is not valid for Encrypt.")
    );
}

#[test]
fn aliases_are_held_to_their_rules() {
    let mut w = World::new();
    let id = w.key(&json!({}));
    let (code, message) = w.err(
        "CreateAlias",
        &json!({"AliasName": "no-prefix", "TargetKeyId": id}),
    );
    assert_eq!(code, "ValidationException");
    assert_eq!(
        message,
        "Alias must start with the prefix \"alias/\". Please see https://docs.aws.amazon.com/kms/latest/developerguide/kms-alias.html"
    );
    w.ok(
        "CreateAlias",
        &json!({"AliasName": "alias/x", "TargetKeyId": id}),
    );
    assert_eq!(
        w.err(
            "CreateAlias",
            &json!({"AliasName": "alias/x", "TargetKeyId": id})
        ),
        (
            "AlreadyExistsException".into(),
            "An alias with the name arn:aws:kms:us-east-1:000000000000:alias/x already exists"
                .into()
        )
    );
    w.ok("DeleteAlias", &json!({"AliasName": "alias/x"}));
    assert_eq!(
        w.err("DeleteAlias", &json!({"AliasName": "alias/x"})),
        (
            "NotFoundException".into(),
            "Alias arn:aws:kms:us-east-1:000000000000:alias/x is not found.".into()
        )
    );
}
