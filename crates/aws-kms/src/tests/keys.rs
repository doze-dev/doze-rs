//! Keys at work: encryption with contexts, data keys, re-encryption, signing, MACs, aliases, rotation, states.

use base64_free::b64;
use serde_json::json;

use super::{START, World};

/// Base64 for the JSON tests without a crate: the blobs travel as base64 text in JSON.
mod base64_free {
    const SET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub(crate) fn b64(bytes: &[u8]) -> String {
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n =
                chunk.iter().fold(0_u32, |a, b| a << 8 | u32::from(*b)) << (8 * (3 - chunk.len()));
            for i in 0..=chunk.len() {
                out.push(char::from(SET[(n >> (18 - 6 * i) & 63) as usize]));
            }
            out.push_str(&"=".repeat(3 - chunk.len()));
        }
        out
    }
}

#[test]
fn a_symmetric_key_encrypts_and_decrypts_with_its_context() {
    let mut w = World::new();
    let id = w.key(&json!({"Description": "conformance"}));
    let sealed = w.ok("Encrypt", &json!({"KeyId": id, "Plaintext": b64(b"attack at dawn"), "EncryptionContext": {"tenant": "a"}}));
    assert_eq!(sealed["EncryptionAlgorithm"], "SYMMETRIC_DEFAULT");
    assert_eq!(
        sealed["KeyId"],
        format!("arn:aws:kms:us-east-1:000000000000:key/{id}")
    );
    let blob = sealed["CiphertextBlob"].clone();
    // No KeyId needed: the blob says whose it is.
    let opened = w.ok(
        "Decrypt",
        &json!({"CiphertextBlob": blob, "EncryptionContext": {"tenant": "a"}}),
    );
    assert_eq!(opened["Plaintext"], b64(b"attack at dawn"));
    assert_eq!(
        w.err("Decrypt", &json!({"CiphertextBlob": blob})).0,
        "InvalidCiphertextException"
    );
    assert_eq!(
        w.err(
            "Decrypt",
            &json!({"CiphertextBlob": blob, "EncryptionContext": {"tenant": "b"}})
        )
        .0,
        "InvalidCiphertextException"
    );
    let other = w.key(&json!({}));
    assert_eq!(
        w.err(
            "Decrypt",
            &json!({"CiphertextBlob": blob, "KeyId": other, "EncryptionContext": {"tenant": "a"}})
        )
        .0,
        "IncorrectKeyException"
    );
}

#[test]
fn data_keys_open_and_reencryption_moves_a_value_between_keys() {
    let mut w = World::new();
    let (a, b) = (w.key(&json!({})), w.key(&json!({})));
    let dk = w.ok(
        "GenerateDataKey",
        &json!({"KeyId": a, "KeySpec": "AES_256"}),
    );
    let sealed = w.ok(
        "GenerateDataKeyWithoutPlaintext",
        &json!({"KeyId": a, "NumberOfBytes": 16}),
    );
    assert!(sealed.get("Plaintext").is_none());
    let opened = w.ok("Decrypt", &json!({"CiphertextBlob": dk["CiphertextBlob"]}));
    assert_eq!(opened["Plaintext"], dk["Plaintext"]);
    assert_eq!(opened["KeyMaterialId"], dk["KeyMaterialId"]);
    let moved = w.ok(
        "ReEncrypt",
        &json!({"CiphertextBlob": dk["CiphertextBlob"], "DestinationKeyId": b}),
    );
    assert!(moved["KeyId"].as_str().unwrap().ends_with(&b));
    assert!(moved["SourceKeyId"].as_str().unwrap().ends_with(&a));
    assert_eq!(
        w.ok(
            "Decrypt",
            &json!({"CiphertextBlob": moved["CiphertextBlob"]})
        )["Plaintext"],
        dk["Plaintext"]
    );
    assert_eq!(
        w.err("GenerateDataKey", &json!({"KeyId": a})).0,
        "ValidationException"
    );
    assert_eq!(
        w.ok("GenerateRandom", &json!({"NumberOfBytes": 10}))["Plaintext"]
            .as_str()
            .unwrap()
            .len(),
        16
    );
}

#[test]
fn aliases_name_keys_and_follow_them() {
    let mut w = World::new();
    let id = w.key(&json!({}));
    w.ok(
        "CreateAlias",
        &json!({"AliasName": "alias/app", "TargetKeyId": id}),
    );
    let via = w.ok("DescribeKey", &json!({"KeyId": "alias/app"}));
    assert_eq!(via["KeyMetadata"]["KeyId"], id.as_str());
    let by_arn = w.ok(
        "DescribeKey",
        &json!({"KeyId": "arn:aws:kms:us-east-1:000000000000:alias/app"}),
    );
    assert_eq!(by_arn["KeyMetadata"]["KeyId"], id.as_str());
    let listed = w.ok("ListAliases", &json!({"KeyId": id}));
    assert_eq!(listed["Aliases"][0]["TargetKeyId"], id.as_str());
    assert_eq!(listed["Truncated"], false);
    let second = w.key(&json!({}));
    w.ok(
        "UpdateAlias",
        &json!({"AliasName": "alias/app", "TargetKeyId": second}),
    );
    assert_eq!(
        w.ok("DescribeKey", &json!({"KeyId": "alias/app"}))["KeyMetadata"]["KeyId"],
        second.as_str()
    );
    w.ok("DeleteAlias", &json!({"AliasName": "alias/app"}));
    assert_eq!(
        w.err("DescribeKey", &json!({"KeyId": "alias/app"})).0,
        "NotFoundException"
    );
}

#[test]
fn a_key_moves_between_states_and_is_removed_when_its_window_ends() {
    let mut w = World::new();
    let id = w.key(&json!({}));
    w.ok(
        "CreateAlias",
        &json!({"AliasName": "alias/gone", "TargetKeyId": id}),
    );
    w.ok("DisableKey", &json!({"KeyId": id}));
    assert_eq!(
        w.err("Encrypt", &json!({"KeyId": id, "Plaintext": b64(b"x")}))
            .0,
        "DisabledException"
    );
    w.ok("EnableKey", &json!({"KeyId": id}));
    let scheduled = w.ok(
        "ScheduleKeyDeletion",
        &json!({"KeyId": id, "PendingWindowInDays": 7}),
    );
    assert_eq!(scheduled["KeyState"], "PendingDeletion");
    assert_eq!(
        w.err("Encrypt", &json!({"KeyId": id, "Plaintext": b64(b"x")}))
            .0,
        "KMSInvalidStateException"
    );
    assert_eq!(
        w.err("EnableKey", &json!({"KeyId": id})).0,
        "KMSInvalidStateException"
    );
    // Cancelling brings the key back disabled (recorded).
    w.ok("CancelKeyDeletion", &json!({"KeyId": id}));
    assert_eq!(
        w.ok("DescribeKey", &json!({"KeyId": id}))["KeyMetadata"]["KeyState"],
        "Disabled"
    );
    w.ok(
        "ScheduleKeyDeletion",
        &json!({"KeyId": id, "PendingWindowInDays": 7}),
    );
    w.clock.set(START + 8 * 86_400_000);
    assert_eq!(
        w.err("DescribeKey", &json!({"KeyId": id})).0,
        "NotFoundException"
    );
    assert_eq!(
        w.ok("ListAliases", &json!({}))["Aliases"],
        json!([]),
        "its aliases went with it"
    );
}

#[test]
fn rotation_keeps_what_the_old_material_sealed() {
    let mut w = World::new();
    let id = w.key(&json!({}));
    let before = w.ok("Encrypt", &json!({"KeyId": id, "Plaintext": b64(b"old")}));
    let first =
        w.ok("DescribeKey", &json!({"KeyId": id}))["KeyMetadata"]["CurrentKeyMaterialId"].clone();
    assert_eq!(
        w.ok("GetKeyRotationStatus", &json!({"KeyId": id})),
        json!({"KeyId": format!("arn:aws:kms:us-east-1:000000000000:key/{id}"), "KeyRotationEnabled": false})
    );
    w.ok("EnableKeyRotation", &json!({"KeyId": id}));
    let status = w.ok("GetKeyRotationStatus", &json!({"KeyId": id}));
    assert_eq!(
        (
            status["RotationPeriodInDays"].as_i64(),
            status["KeyRotationEnabled"].clone()
        ),
        (Some(365), json!(true))
    );
    w.ok("RotateKeyOnDemand", &json!({"KeyId": id}));
    let second =
        w.ok("DescribeKey", &json!({"KeyId": id}))["KeyMetadata"]["CurrentKeyMaterialId"].clone();
    assert_ne!(first, second);
    assert_eq!(
        w.ok(
            "Decrypt",
            &json!({"CiphertextBlob": before["CiphertextBlob"]})
        )["Plaintext"],
        b64(b"old")
    );
    let fresh = w.ok("Encrypt", &json!({"KeyId": id, "Plaintext": b64(b"new")}));
    assert_eq!(
        w.ok(
            "Decrypt",
            &json!({"CiphertextBlob": fresh["CiphertextBlob"]})
        )["KeyMaterialId"],
        second
    );
    assert_eq!(
        w.ok("ListKeyRotations", &json!({"KeyId": id}))["Rotations"][0]["RotationType"],
        "ON_DEMAND"
    );
    w.ok("DisableKeyRotation", &json!({"KeyId": id}));
    let signer = w.key(&json!({"KeySpec": "ECC_NIST_P256", "KeyUsage": "SIGN_VERIFY"}));
    assert_eq!(
        w.err("EnableKeyRotation", &json!({"KeyId": signer})).0,
        "UnsupportedOperationException"
    );
}

#[test]
fn signing_and_macs_with_every_kind_of_key() {
    let mut w = World::new();
    for (spec, algorithm) in [
        ("ECC_NIST_P256", "ECDSA_SHA_256"),
        ("ECC_NIST_P384", "ECDSA_SHA_384"),
        ("ECC_NIST_P521", "ECDSA_SHA_512"),
        ("ECC_SECG_P256K1", "ECDSA_SHA_256"),
        ("ECC_NIST_EDWARDS25519", "ED25519_SHA_512"),
    ] {
        let id = w.key(&json!({"KeySpec": spec, "KeyUsage": "SIGN_VERIFY"}));
        let signed = w.ok(
            "Sign",
            &json!({"KeyId": id, "Message": b64(b"msg"), "SigningAlgorithm": algorithm}),
        );
        let check = json!({"KeyId": id, "Message": b64(b"msg"), "Signature": signed["Signature"], "SigningAlgorithm": algorithm});
        assert_eq!(w.ok("Verify", &check)["SignatureValid"], true, "{spec}");
        let tampered = json!({"KeyId": id, "Message": b64(b"other"), "Signature": signed["Signature"], "SigningAlgorithm": algorithm});
        assert_eq!(
            w.err("Verify", &tampered),
            ("KMSInvalidSignatureException".into(), String::new()),
            "{spec}"
        );
        assert!(
            !w.ok("GetPublicKey", &json!({"KeyId": id}))["PublicKey"]
                .as_str()
                .unwrap()
                .is_empty()
        );
    }
    let mac_key = w.key(&json!({"KeySpec": "HMAC_256", "KeyUsage": "GENERATE_VERIFY_MAC"}));
    let tag = w.ok(
        "GenerateMac",
        &json!({"KeyId": mac_key, "Message": b64(b"msg"), "MacAlgorithm": "HMAC_SHA_256"}),
    );
    let check = json!({"KeyId": mac_key, "Message": b64(b"msg"), "Mac": tag["Mac"], "MacAlgorithm": "HMAC_SHA_256"});
    assert_eq!(w.ok("VerifyMac", &check)["MacValid"], true);
    let bad = json!({"KeyId": mac_key, "Message": b64(b"other"), "Mac": tag["Mac"], "MacAlgorithm": "HMAC_SHA_256"});
    assert_eq!(w.err("VerifyMac", &bad).0, "KMSInvalidMacException");
    assert_eq!(
        w.err(
            "GenerateMac",
            &json!({"KeyId": mac_key, "Message": b64(b"m"), "MacAlgorithm": "HMAC_SHA_512"})
        )
        .0,
        "InvalidKeyUsageException"
    );
}

#[test]
fn policies_and_tags_round_trip() {
    let mut w = World::new();
    let id = w.key(&json!({"Tags": [{"TagKey": "env", "TagValue": "dev"}]}));
    let default = w.ok("GetKeyPolicy", &json!({"KeyId": id}));
    assert!(
        default["Policy"]
            .as_str()
            .unwrap()
            .contains("arn:aws:iam::000000000000:root")
    );
    w.ok("PutKeyPolicy", &json!({"KeyId": id, "PolicyName": "default", "Policy": "{\"Version\":\"2012-10-17\",\"Statement\":[]}"}));
    assert_eq!(
        w.ok("GetKeyPolicy", &json!({"KeyId": id}))["Policy"],
        "{\"Version\":\"2012-10-17\",\"Statement\":[]}"
    );
    assert_eq!(
        w.err("PutKeyPolicy", &json!({"KeyId": id, "Policy": "nope"}))
            .0,
        "MalformedPolicyDocumentException"
    );
    assert_eq!(
        w.ok("ListKeyPolicies", &json!({"KeyId": id}))["PolicyNames"],
        json!(["default"])
    );
    w.ok(
        "TagResource",
        &json!({"KeyId": id, "Tags": [{"TagKey": "team", "TagValue": "x"}]}),
    );
    w.ok("UntagResource", &json!({"KeyId": id, "TagKeys": ["env"]}));
    assert_eq!(
        w.ok("ListResourceTags", &json!({"KeyId": id}))["Tags"],
        json!([{"TagKey": "team", "TagValue": "x"}])
    );
    w.ok(
        "UpdateKeyDescription",
        &json!({"KeyId": id, "Description": "new"}),
    );
    assert_eq!(
        w.ok("DescribeKey", &json!({"KeyId": id}))["KeyMetadata"]["Description"],
        "new"
    );
    assert_eq!(
        w.ok("ListKeys", &json!({}))["Keys"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
