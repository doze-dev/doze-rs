//! RSA keys: made, kept as a PKCS #8 document, and used for OAEP encryption, PSS signatures and PKCS #1 v1.5 signatures.
//! The `rsa` crate is a release candidate (pinned exactly) and has a timing advisory (RUSTSEC-2023-0071) that is acceptable for a
//! local development tool and is noted in `docs/port/aws/PLAN.md`.

use getrandom::SysRng;
use rsa::PrivateKey;
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey};
use rsa::sha2::{Sha256, Sha384, Sha512};
use sha1::Sha1;

use crate::asym::Fail;

/// A new key of `bits`, as a PKCS #8 `PrivateKeyInfo`.
pub(crate) fn generate(bits: usize) -> Option<Vec<u8>> {
    let key = PrivateKey::new(&mut SysRng, bits).ok()?;
    Some(key.to_pkcs8_der().ok()?.as_bytes().to_vec())
}

fn load(der: &[u8]) -> Option<PrivateKey> {
    PrivateKey::from_pkcs8_der(der).ok()
}

/// The private key as it was kept (a data key pair hands it out).
pub(crate) fn private_key(der: &[u8]) -> Option<Vec<u8>> {
    load(der)?;
    Some(der.to_vec())
}

/// The public key as a DER `SubjectPublicKeyInfo`.
pub(crate) fn public_key(der: &[u8]) -> Option<Vec<u8>> {
    Some(
        load(der)?
            .to_public_key()
            .to_public_key_der()
            .ok()?
            .as_bytes()
            .to_vec(),
    )
}

/// How many bytes an RSA ciphertext or signature of this key has.
pub(crate) fn modulus_len(der: &[u8]) -> Option<usize> {
    use rsa::traits::PublicKeyParts;
    Some(load(der)?.size())
}

macro_rules! by_hash {
    ($alg:expr, $hash:ident => $body:expr, $other:expr) => {
        match $alg {
            "RSASSA_PSS_SHA_256" | "RSASSA_PKCS1_V1_5_SHA_256" | "RSAES_OAEP_SHA_256" => {
                type $hash = Sha256;
                $body
            }
            "RSASSA_PSS_SHA_384" | "RSASSA_PKCS1_V1_5_SHA_384" => {
                type $hash = Sha384;
                $body
            }
            "RSASSA_PSS_SHA_512" | "RSASSA_PKCS1_V1_5_SHA_512" => {
                type $hash = Sha512;
                $body
            }
            _ => $other,
        }
    };
}

/// Signs `message` (or, with `digest`, a digest) under `algorithm`.
pub(crate) fn sign(algorithm: &str, der: &[u8], message: &[u8], digest: bool) -> Option<Vec<u8>> {
    let key = load(der)?;
    let pss = algorithm.starts_with("RSASSA_PSS");
    by_hash!(algorithm, H => {
        match (pss, digest) {
            (true, false) => key.pss_sign::<H>(message),
            (true, true) => key.pss_sign_prehash::<H>(message),
            (false, false) => key.pkcs1v15_sign::<H>(message),
            (false, true) => key.pkcs1v15_sign_prehash::<H>(message),
        }
        .ok()
    }, None)
}

/// Verifies a signature.
pub(crate) fn verify(
    algorithm: &str,
    der: &[u8],
    message: &[u8],
    digest: bool,
    signature: &[u8],
) -> Result<(), Fail> {
    let key = load(der).ok_or(Fail::Malformed)?.to_public_key();
    let pss = algorithm.starts_with("RSASSA_PSS");
    let result = by_hash!(algorithm, H => {
        match (pss, digest) {
            (true, false) => key.pss_verify::<H>(message, signature),
            (true, true) => key.pss_verify_prehash::<H>(message, signature),
            (false, false) => key.pkcs1v15_verify::<H>(message, signature),
            (false, true) => key.pkcs1v15_verify_prehash::<H>(message, signature),
        }
    }, return Err(Fail::Malformed));
    result.map_err(|_| Fail::Invalid)
}

/// OAEP-encrypts under the public half.
pub(crate) fn encrypt(algorithm: &str, der: &[u8], plain: &[u8]) -> Option<Vec<u8>> {
    let key = load(der)?.to_public_key();
    match algorithm {
        "RSAES_OAEP_SHA_1" => key.oaep_encrypt::<Sha1>(plain).ok(),
        "RSAES_OAEP_SHA_256" => key.oaep_encrypt::<Sha256>(plain).ok(),
        _ => None,
    }
}

/// OAEP-decrypts.
pub(crate) fn decrypt(algorithm: &str, der: &[u8], cipher: &[u8]) -> Option<Vec<u8>> {
    let key = load(der)?;
    match algorithm {
        "RSAES_OAEP_SHA_1" => key.oaep_decrypt::<Sha1>(cipher).ok(),
        "RSAES_OAEP_SHA_256" => key.oaep_decrypt::<Sha256>(cipher).ok(),
        _ => None,
    }
}

/// OAEP-encrypts under a public key given as a DER `SubjectPublicKeyInfo`: what a caller does with the key `GetParametersForImport` hands out.
#[cfg(test)]
pub(crate) fn encrypt_to_public(algorithm: &str, spki: &[u8], plain: &[u8]) -> Option<Vec<u8>> {
    use rsa::pkcs8::DecodePublicKey;
    let key = rsa::PublicKey::from_public_key_der(spki).ok()?;
    match algorithm {
        "RSAES_OAEP_SHA_1" => key.oaep_encrypt::<Sha1>(plain).ok(),
        "RSAES_OAEP_SHA_256" => key.oaep_encrypt::<Sha256>(plain).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn a_key_signs_encrypts_and_refuses_a_change() {
        let der = generate(2048).unwrap();
        assert!(private_key(&der).is_some());
        for alg in [
            "RSASSA_PSS_SHA_256",
            "RSASSA_PSS_SHA_512",
            "RSASSA_PKCS1_V1_5_SHA_384",
        ] {
            let sig = sign(alg, &der, b"msg", false).unwrap();
            assert_eq!(verify(alg, &der, b"msg", false, &sig), Ok(()), "{alg}");
            assert_eq!(
                verify(alg, &der, b"other", false, &sig),
                Err(Fail::Invalid),
                "{alg}"
            );
        }
        for alg in ["RSAES_OAEP_SHA_1", "RSAES_OAEP_SHA_256"] {
            let sealed = encrypt(alg, &der, b"attack at dawn").unwrap();
            assert_eq!(
                decrypt(alg, &der, &sealed).as_deref(),
                Some(&b"attack at dawn"[..]),
                "{alg}"
            );
            assert_eq!(decrypt(alg, &der, b"garbage"), None);
        }
        // Too long for the modulus.
        assert_eq!(encrypt("RSAES_OAEP_SHA_256", &der, &[0; 300]), None);
        assert!(public_key(&der).unwrap().len() > 256);
    }
}
