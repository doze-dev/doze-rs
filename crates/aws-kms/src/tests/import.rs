//! Imported key material.

use aes_kw::{KeyInit, KwpAes256};
use serde_json::json;

use super::keys::b64;
use super::{START, World};
use crate::rsa_keys::encrypt_to_public;

fn unb64(text: &str) -> Vec<u8> {
    super::keys::unb64(text)
}

const MATERIAL: [u8; 32] = [0x42; 32];

/// What a caller sends: the 32 bytes wrapped as `algorithm` says, with the wrapping key the import handed out.
fn wrapped(algorithm: &str, public: &[u8]) -> Vec<u8> {
    let oaep = if algorithm.ends_with("SHA_1") {
        "RSAES_OAEP_SHA_1"
    } else {
        "RSAES_OAEP_SHA_256"
    };
    if !algorithm.starts_with("RSA_AES_KEY_WRAP") {
        return encrypt_to_public(oaep, public, &MATERIAL).unwrap();
    }
    let aes = [0x17_u8; 32];
    let mut out = encrypt_to_public(oaep, public, &aes).unwrap();
    let mut buf = [0_u8; 48];
    out.extend_from_slice(
        KwpAes256::new_from_slice(&aes)
            .unwrap()
            .wrap_key(&MATERIAL, &mut buf)
            .unwrap(),
    );
    out
}

fn import(
    w: &mut World,
    id: &str,
    algorithm: &str,
    extra: serde_json::Value,
) -> Result<serde_json::Value, (String, String)> {
    let p = w.ok(
        "GetParametersForImport",
        &json!({"KeyId": id, "WrappingAlgorithm": algorithm, "WrappingKeySpec": "RSA_2048"}),
    );
    let blob = b64(&wrapped(
        algorithm,
        &unb64(p["PublicKey"].as_str().unwrap()),
    ));
    let mut ask =
        json!({"KeyId": id, "ImportToken": p["ImportToken"], "EncryptedKeyMaterial": blob});
    ask.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    w.run("ImportKeyMaterial", &ask).map_err(|e| {
        (
            e.shape
                .map_or_else(|| e.code.to_string(), |s| s.to_string()),
            e.message,
        )
    })
}

#[test]
fn material_imported_under_each_wrapping_algorithm_works_as_a_key() {
    for algorithm in [
        "RSAES_OAEP_SHA_1",
        "RSAES_OAEP_SHA_256",
        "RSA_AES_KEY_WRAP_SHA_1",
        "RSA_AES_KEY_WRAP_SHA_256",
    ] {
        let mut w = World::new();
        let id = w.key(&json!({"Origin": "EXTERNAL"}));
        let meta = w.ok("DescribeKey", &json!({"KeyId": id}))["KeyMetadata"].clone();
        assert_eq!(
            (
                meta["KeyState"].as_str(),
                meta["Origin"].as_str(),
                meta["Enabled"].clone()
            ),
            (Some("PendingImport"), Some("EXTERNAL"), json!(false))
        );
        assert_eq!(
            w.err("Encrypt", &json!({"KeyId": id, "Plaintext": "eA=="}))
                .0,
            "KMSInvalidStateException"
        );
        let done = import(&mut w, &id, algorithm, json!({})).unwrap();
        assert_eq!(
            done["KeyMaterialId"].as_str().unwrap().len(),
            64,
            "{algorithm}"
        );
        assert_eq!(
            w.ok("DescribeKey", &json!({"KeyId": id}))["KeyMetadata"]["KeyState"],
            "Enabled"
        );
        let sealed = w.ok(
            "Encrypt",
            &json!({"KeyId": id, "Plaintext": b64(b"imported")}),
        );
        assert_eq!(
            w.ok(
                "Decrypt",
                &json!({"CiphertextBlob": sealed["CiphertextBlob"]})
            )["Plaintext"],
            b64(b"imported")
        );
    }
}

#[test]
fn deleting_the_material_waits_for_the_same_material_to_come_back() {
    let mut w = World::new();
    let id = w.key(&json!({"Origin": "EXTERNAL"}));
    import(&mut w, &id, "RSAES_OAEP_SHA_256", json!({})).unwrap();
    let sealed = w.ok("Encrypt", &json!({"KeyId": id, "Plaintext": b64(b"keep")}));
    w.ok("DeleteImportedKeyMaterial", &json!({"KeyId": id}));
    assert_eq!(
        w.ok("DescribeKey", &json!({"KeyId": id}))["KeyMetadata"]["KeyState"],
        "PendingImport"
    );
    assert_eq!(
        w.err(
            "Decrypt",
            &json!({"CiphertextBlob": sealed["CiphertextBlob"]})
        )
        .0,
        "KMSInvalidStateException"
    );
    import(&mut w, &id, "RSAES_OAEP_SHA_256", json!({})).unwrap();
    assert_eq!(
        w.ok(
            "Decrypt",
            &json!({"CiphertextBlob": sealed["CiphertextBlob"]})
        )["Plaintext"],
        b64(b"keep")
    );
}

#[test]
fn material_that_expires_takes_the_key_back_to_pending_import() {
    let mut w = World::new();
    let id = w.key(&json!({"Origin": "EXTERNAL"}));
    let at = (START / 1000 + 3600) as f64;
    import(
        &mut w,
        &id,
        "RSAES_OAEP_SHA_256",
        json!({"ExpirationModel": "KEY_MATERIAL_EXPIRES", "ValidTo": at}),
    )
    .unwrap();
    let meta = w.ok("DescribeKey", &json!({"KeyId": id}))["KeyMetadata"].clone();
    assert_eq!(meta["ExpirationModel"], "KEY_MATERIAL_EXPIRES");
    w.clock.set(START + 2 * 3_600_000);
    assert_eq!(
        w.ok("DescribeKey", &json!({"KeyId": id}))["KeyMetadata"]["KeyState"],
        "PendingImport"
    );
}

#[test]
fn imports_are_held_to_their_rules() {
    let mut w = World::new();
    let aws = w.key(&json!({}));
    assert_eq!(w.err("GetParametersForImport", &json!({"KeyId": aws, "WrappingAlgorithm": "RSAES_OAEP_SHA_256", "WrappingKeySpec": "RSA_2048"})).0, "UnsupportedOperationException");
    assert_eq!(
        w.err(
            "CreateKey",
            &json!({"Origin": "EXTERNAL", "KeySpec": "ECC_NIST_P256", "KeyUsage": "SIGN_VERIFY"})
        )
        .0,
        "ValidationException"
    );
    let id = w.key(&json!({"Origin": "EXTERNAL"}));
    assert_eq!(
        w.err(
            "ImportKeyMaterial",
            &json!({"KeyId": id, "ImportToken": "dG9rZW4=", "EncryptedKeyMaterial": "eA=="})
        )
        .0,
        "InvalidImportTokenException"
    );
    let p = w.ok("GetParametersForImport", &json!({"KeyId": id, "WrappingAlgorithm": "RSAES_OAEP_SHA_256", "WrappingKeySpec": "RSA_2048"}));
    assert_eq!(
        w.err(
            "ImportKeyMaterial",
            &json!({"KeyId": id, "ImportToken": "d3Jvbmc=", "EncryptedKeyMaterial": "eA=="})
        )
        .0,
        "InvalidImportTokenException"
    );
    assert_eq!(
        w.err(
            "ImportKeyMaterial",
            &json!({"KeyId": id, "ImportToken": p["ImportToken"], "EncryptedKeyMaterial": "eA=="})
        )
        .0,
        "IncorrectKeyMaterialException"
    );
    // A day later the token has expired.
    w.clock.set(START + 25 * 3_600_000);
    let blob = b64(&wrapped(
        "RSAES_OAEP_SHA_256",
        &unb64(p["PublicKey"].as_str().unwrap()),
    ));
    assert_eq!(
        w.err(
            "ImportKeyMaterial",
            &json!({"KeyId": id, "ImportToken": p["ImportToken"], "EncryptedKeyMaterial": blob})
        )
        .0,
        "ExpiredImportTokenException"
    );
}
