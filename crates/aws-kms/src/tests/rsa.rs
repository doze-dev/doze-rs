//! RSA keys: made when first used, signing, OAEP, public keys, key pairs.

use serde_json::json;

use super::World;

/// Whether the key has had its material made.
fn generated(w: &World, id: &str) -> bool {
    !w.core
        .keys
        .get(id)
        .unwrap()
        .current()
        .unwrap()
        .sealed
        .is_empty()
}

#[test]
fn an_rsa_signing_key_signs_with_every_algorithm_it_lists() {
    let mut w = World::new();
    let id = w.key(&json!({"KeySpec": "RSA_2048", "KeyUsage": "SIGN_VERIFY"}));
    let meta = w.ok("DescribeKey", &json!({"KeyId": id}));
    assert_eq!(
        meta["KeyMetadata"]["SigningAlgorithms"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    assert!(meta["KeyMetadata"].get("EncryptionAlgorithms").is_none());
    // Nothing has been generated yet: the first use does it, and a second use keeps the same key.
    assert!(!generated(&w, &id));
    let first = w.ok("GetPublicKey", &json!({"KeyId": id}));
    assert!(generated(&w, &id));
    assert_eq!(
        w.ok("GetPublicKey", &json!({"KeyId": id}))["PublicKey"],
        first["PublicKey"]
    );
}

#[test]
fn every_rsa_signature_algorithm_signs_verifies_and_refuses_a_change() {
    let mut w = World::new();
    let id = w.key(&json!({"KeySpec": "RSA_2048", "KeyUsage": "SIGN_VERIFY"}));
    for algorithm in [
        "RSASSA_PSS_SHA_256",
        "RSASSA_PSS_SHA_512",
        "RSASSA_PKCS1_V1_5_SHA_256",
        "RSASSA_PKCS1_V1_5_SHA_384",
    ] {
        let signed = w.ok(
            "Sign",
            &json!({"KeyId": id, "Message": "bXNn", "SigningAlgorithm": algorithm}),
        );
        let check = json!({"KeyId": id, "Message": "bXNn", "Signature": signed["Signature"], "SigningAlgorithm": algorithm});
        assert_eq!(
            w.ok("Verify", &check)["SignatureValid"],
            true,
            "{algorithm}"
        );
        let bad = json!({"KeyId": id, "Message": "b3RoZXI=", "Signature": signed["Signature"], "SigningAlgorithm": algorithm});
        assert_eq!(
            w.err("Verify", &bad).0,
            "KMSInvalidSignatureException",
            "{algorithm}"
        );
    }
    assert_eq!(
        w.err(
            "Sign",
            &json!({"KeyId": id, "Message": "bQ==", "SigningAlgorithm": "ECDSA_SHA_256"})
        )
        .0,
        "InvalidKeyUsageException"
    );
    assert_eq!(
        w.err(
            "Encrypt",
            &json!({"KeyId": id, "Plaintext": "eA==", "EncryptionAlgorithm": "RSAES_OAEP_SHA_256"})
        )
        .0,
        "InvalidKeyUsageException"
    );
}

#[test]
fn an_rsa_encryption_key_does_oaep() {
    let mut w = World::new();
    let id = w.key(&json!({"KeySpec": "RSA_2048", "KeyUsage": "ENCRYPT_DECRYPT"}));
    let meta = w.ok("DescribeKey", &json!({"KeyId": id}));
    assert_eq!(
        meta["KeyMetadata"]["EncryptionAlgorithms"],
        json!(["RSAES_OAEP_SHA_1", "RSAES_OAEP_SHA_256"])
    );
    for algorithm in ["RSAES_OAEP_SHA_1", "RSAES_OAEP_SHA_256"] {
        let sealed = w.ok(
            "Encrypt",
            &json!({"KeyId": id, "Plaintext": "YXR0YWNr", "EncryptionAlgorithm": algorithm}),
        );
        assert_eq!(sealed["EncryptionAlgorithm"], algorithm);
        let opened = w.ok("Decrypt", &json!({"CiphertextBlob": sealed["CiphertextBlob"], "KeyId": id, "EncryptionAlgorithm": algorithm}));
        assert_eq!(opened["Plaintext"], "YXR0YWNr");
    }
    assert_eq!(
        w.err("Encrypt", &json!({"KeyId": id, "Plaintext": "eA=="}))
            .0,
        "ValidationException",
        "an algorithm is required"
    );
    assert_eq!(w.err("Decrypt", &json!({"CiphertextBlob": "Z2FyYmFnZQ==", "KeyId": id, "EncryptionAlgorithm": "RSAES_OAEP_SHA_256"})).0, "InvalidCiphertextException");
    let long = "eA".repeat(200) + "==";
    assert_eq!(
        w.err(
            "Encrypt",
            &json!({"KeyId": id, "Plaintext": long, "EncryptionAlgorithm": "RSAES_OAEP_SHA_256"})
        )
        .0,
        "ValidationException"
    );
}

#[test]
fn an_rsa_key_pair_is_handed_out_and_the_material_survives_a_restart() {
    let mut w = World::new();
    let sym = w.key(&json!({}));
    let pair = w.ok(
        "GenerateDataKeyPair",
        &json!({"KeyId": sym, "KeyPairSpec": "RSA_2048"}),
    );
    assert!(!pair["PublicKey"].as_str().unwrap().is_empty());
    let opened = w.ok(
        "Decrypt",
        &json!({"CiphertextBlob": pair["PrivateKeyCiphertextBlob"]}),
    );
    assert_eq!(opened["Plaintext"], pair["PrivateKeyPlaintext"]);

    let dir = super::temp();
    let clock = std::sync::Arc::new(doze_runtime::ManualClock::at(super::START));
    let mut on_disk = World::with(
        crate::core::Core::open_in(dir.path(), clock.clone()).unwrap(),
        clock.clone(),
    );
    let id = on_disk.key(&json!({"KeySpec": "RSA_2048", "KeyUsage": "SIGN_VERIFY"}));
    let public = on_disk.ok("GetPublicKey", &json!({"KeyId": id}))["PublicKey"].clone();
    on_disk.core.db.take().unwrap().close();
    on_disk.core = crate::core::Core::open_in(dir.path(), clock).unwrap();
    assert_eq!(
        on_disk.ok("GetPublicKey", &json!({"KeyId": id}))["PublicKey"],
        public
    );
}
