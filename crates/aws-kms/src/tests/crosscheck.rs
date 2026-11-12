//! KMS's cryptography checked against a second implementation, `ring`, which shares no code with RustCrypto: what KMS signs, `ring`
//! verifies; what KMS MACs, `ring` MACs the same; what KMS seals, `ring` opens. Together with the known-answer tests next to the code
//! (RFC 4231, RFC 8032) this is the cross-check the plan asked for. OAEP encryption is not checked this way: `ring` has no RSA encryption.

use ring::{aead, hmac, signature};
use serde_json::json;

use super::World;
use super::keys::{b64, unb64};

/// The bytes of the bit string at the end of a DER `SubjectPublicKeyInfo`: the public key proper (a curve point, a 32-byte Ed25519
/// key, or a DER `RSAPublicKey`).
fn key_bits(spki: &[u8]) -> Vec<u8> {
    fn header(der: &[u8]) -> (usize, usize) {
        // tag, then a length that is short (one byte) or long (0x80 | n, then n bytes): returns where the content starts and its length
        let first = usize::from(der[1]);
        if first < 0x80 {
            (2, first)
        } else {
            let n = first & 0x7f;
            (
                2 + n,
                der[2..2 + n]
                    .iter()
                    .fold(0, |acc, b| acc << 8 | usize::from(*b)),
            )
        }
    }
    let (start, _) = header(spki);
    let inner = &spki[start..];
    let (alg_start, alg_len) = header(inner);
    let rest = &inner[alg_start + alg_len..];
    let (bit_start, bit_len) = header(rest);
    // The first byte of a bit string says how many bits of the last byte are unused: none.
    rest[bit_start + 1..bit_start + bit_len].to_vec()
}

#[test]
fn ring_verifies_what_kms_signs() {
    let mut w = World::new();
    for (spec, algorithm, verifier) in [
        (
            "ECC_NIST_P256",
            "ECDSA_SHA_256",
            &signature::ECDSA_P256_SHA256_ASN1 as &dyn signature::VerificationAlgorithm,
        ),
        (
            "ECC_NIST_P384",
            "ECDSA_SHA_384",
            &signature::ECDSA_P384_SHA384_ASN1,
        ),
        (
            "ECC_NIST_EDWARDS25519",
            "ED25519_SHA_512",
            &signature::ED25519,
        ),
        (
            "RSA_2048",
            "RSASSA_PSS_SHA_256",
            &signature::RSA_PSS_2048_8192_SHA256,
        ),
        (
            "RSA_2048",
            "RSASSA_PKCS1_V1_5_SHA_256",
            &signature::RSA_PKCS1_2048_8192_SHA256,
        ),
        (
            "RSA_2048",
            "RSASSA_PKCS1_V1_5_SHA_384",
            &signature::RSA_PKCS1_2048_8192_SHA384,
        ),
        (
            "RSA_2048",
            "RSASSA_PSS_SHA_512",
            &signature::RSA_PSS_2048_8192_SHA512,
        ),
    ] {
        let id = w.key(&json!({"KeySpec": spec, "KeyUsage": "SIGN_VERIFY"}));
        let public = unb64(
            w.ok("GetPublicKey", &json!({"KeyId": id}))["PublicKey"]
                .as_str()
                .unwrap(),
        );
        let signed = w.ok(
            "Sign",
            &json!({"KeyId": id, "Message": b64(b"cross-check"), "SigningAlgorithm": algorithm}),
        );
        let sig = unb64(signed["Signature"].as_str().unwrap());
        let key = signature::UnparsedPublicKey::new(verifier, key_bits(&public));
        assert!(
            key.verify(b"cross-check", &sig).is_ok(),
            "{spec} {algorithm}"
        );
        assert!(
            key.verify(b"cross-checK", &sig).is_err(),
            "{spec} {algorithm}: a changed message must not verify"
        );
    }
}

#[test]
fn ring_computes_the_same_macs() {
    let mut w = World::new();
    for (spec, algorithm, ring_algorithm) in [
        ("HMAC_256", "HMAC_SHA_256", hmac::HMAC_SHA256),
        ("HMAC_384", "HMAC_SHA_384", hmac::HMAC_SHA384),
        ("HMAC_512", "HMAC_SHA_512", hmac::HMAC_SHA512),
    ] {
        let id = w.key(&json!({"KeySpec": spec, "KeyUsage": "GENERATE_VERIFY_MAC"}));
        let tag = unb64(
            w.ok(
                "GenerateMac",
                &json!({"KeyId": id, "Message": b64(b"cross-check"), "MacAlgorithm": algorithm}),
            )["Mac"]
                .as_str()
                .unwrap(),
        );
        let material = w
            .core
            .keys
            .get(&id)
            .unwrap()
            .current()
            .unwrap()
            .sealed
            .clone();
        let key = w.core.sealer.open(&material).unwrap();
        let expected = hmac::sign(&hmac::Key::new(ring_algorithm, &key), b"cross-check");
        assert_eq!(tag, expected.as_ref(), "{algorithm}");
    }
}

#[test]
fn ring_opens_what_kms_seals() {
    let mut w = World::new();
    let id = w.key(&json!({}));
    let sealed = w.ok(
        "Encrypt",
        &json!({"KeyId": id, "Plaintext": b64(b"cross-check"), "EncryptionContext": {"a": "b"}}),
    );
    let blob = unb64(sealed["CiphertextBlob"].as_str().unwrap());
    // 0x01 | id length | id | material id (32) | nonce (12) | ciphertext and tag
    let id_len = usize::from(blob[1]);
    let nonce_at = 2 + id_len + 32;
    let nonce: [u8; 12] = blob[nonce_at..nonce_at + 12].try_into().unwrap();
    let mut body = blob[nonce_at + 12..].to_vec();
    let material = w
        .core
        .keys
        .get(&id)
        .unwrap()
        .current()
        .unwrap()
        .sealed
        .clone();
    let key = aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_256_GCM, &w.core.sealer.open(&material).unwrap()).unwrap(),
    );
    // The associated data is the context as KMS writes it: each key and value ended by a zero.
    let opened = key
        .open_in_place(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::from(b"a\0b\0".to_vec()),
            &mut body,
        )
        .unwrap();
    assert_eq!(opened, b"cross-check");
}
