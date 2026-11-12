//! Grants and data key pairs.

use serde_json::json;

use super::World;

#[test]
fn grants_are_kept_listed_and_retired() {
    let mut w = World::new();
    let id = w.key(&json!({}));
    let who = "arn:aws:iam::000000000000:role/app";
    let made = w.ok("CreateGrant", &json!({"KeyId": id, "GranteePrincipal": who, "RetiringPrincipal": who, "Operations": ["Decrypt", "Encrypt"], "Name": "g1", "Constraints": {"EncryptionContextSubset": {"a": "b"}}}));
    let (gid, token) = (
        made["GrantId"].as_str().unwrap().to_owned(),
        made["GrantToken"].as_str().unwrap().to_owned(),
    );
    // The same name on the same key is the same grant.
    let again = w.ok(
        "CreateGrant",
        &json!({"KeyId": id, "GranteePrincipal": who, "Operations": ["Decrypt"], "Name": "g1"}),
    );
    assert_eq!(again["GrantId"], gid.as_str());
    let listed = w.ok("ListGrants", &json!({"KeyId": id}));
    assert_eq!(listed["Grants"].as_array().unwrap().len(), 1);
    assert_eq!(
        listed["Grants"][0]["Operations"],
        json!(["Decrypt", "Encrypt"])
    );
    assert_eq!(
        listed["Grants"][0]["Constraints"]["EncryptionContextSubset"],
        json!({"a": "b"})
    );
    assert_eq!(
        w.ok("ListRetirableGrants", &json!({"RetiringPrincipal": who}))["Grants"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        w.err(
            "CreateGrant",
            &json!({"KeyId": id, "GranteePrincipal": who, "Operations": ["Nonsense"]})
        )
        .0,
        "ValidationException"
    );
    w.ok("RetireGrant", &json!({"GrantToken": token}));
    assert_eq!(
        w.ok("ListGrants", &json!({"KeyId": id}))["Grants"],
        json!([])
    );
    let second = w.ok(
        "CreateGrant",
        &json!({"KeyId": id, "GranteePrincipal": who, "Operations": ["Sign"]}),
    );
    w.ok(
        "RevokeGrant",
        &json!({"KeyId": id, "GrantId": second["GrantId"]}),
    );
    assert_eq!(
        w.err(
            "RevokeGrant",
            &json!({"KeyId": id, "GrantId": second["GrantId"]})
        )
        .0,
        "NotFoundException"
    );
}

#[test]
fn a_data_key_pair_is_a_pair() {
    use p256::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePublicKey};
    let mut w = World::new();
    let id = w.key(&json!({}));
    let pair = w.ok(
        "GenerateDataKeyPair",
        &json!({"KeyId": id, "KeyPairSpec": "ECC_NIST_P256"}),
    );
    let b = |v: &serde_json::Value| super::keys::unb64(v.as_str().unwrap());
    let secret = p256::SecretKey::from_pkcs8_der(&b(&pair["PrivateKeyPlaintext"])).unwrap();
    let public = p256::PublicKey::from_public_key_der(&b(&pair["PublicKey"])).unwrap();
    assert_eq!(
        secret.public_key().to_public_key_der().unwrap(),
        public.to_public_key_der().unwrap()
    );
    let opened = w.ok(
        "Decrypt",
        &json!({"CiphertextBlob": pair["PrivateKeyCiphertextBlob"]}),
    );
    assert_eq!(opened["Plaintext"], pair["PrivateKeyPlaintext"]);
    let sealed = w.ok(
        "GenerateDataKeyPairWithoutPlaintext",
        &json!({"KeyId": id, "KeyPairSpec": "ECC_NIST_P384"}),
    );
    assert!(sealed.get("PrivateKeyPlaintext").is_none());
    assert_eq!(
        w.err(
            "GenerateDataKeyPair",
            &json!({"KeyId": id, "KeyPairSpec": "SM2"})
        )
        .0,
        "UnsupportedOperationException"
    );
}
