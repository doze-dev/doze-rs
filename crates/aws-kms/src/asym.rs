//! The asymmetric half of KMS: making key material, public keys, signing and verifying. ECDSA signatures are DER, as KMS makes
//! them. A curve has one hash (P-256 and secp256k1 SHA-256, P-384 SHA-384, P-521 SHA-512), which is why each has one algorithm.

use sha2::{Digest, Sha256, Sha384, Sha512};

use crate::spec::Spec;

/// Why a verification did not succeed.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Fail {
    /// The signature does not match.
    Invalid,
    /// The key, the digest or the signature is not in a form this can use.
    Malformed,
}

/// Material for a new key of `spec`: random bytes, drawn again until the curve accepts them as a private key.
pub(crate) fn generate(spec: Spec) -> Option<Vec<u8>> {
    for _ in 0..16 {
        let mut bytes = vec![0_u8; spec.material_len()];
        getrandom_fill(&mut bytes)?;
        // The top of a P-521 scalar has only one significant bit.
        if spec == Spec::EccP521
            && let Some(first) = bytes.first_mut()
        {
            *first &= 1;
        }
        if spec.is_hmac()
            || spec == Spec::Symmetric
            || spec == Spec::Ed25519
            || public_key(spec, &bytes).is_some()
        {
            return Some(bytes);
        }
    }
    None
}

fn getrandom_fill(bytes: &mut [u8]) -> Option<()> {
    for chunk in bytes.chunks_mut(32) {
        let r = aws_seal::random_bytes::<32>()?;
        chunk.copy_from_slice(r.get(..chunk.len())?);
    }
    Some(())
}

macro_rules! ecdsa {
    ($curve:ident, $hash:ty) => {
        pub(crate) mod $curve {
            use ::$curve::ecdsa::signature::hazmat::{PrehashSigner, PrehashVerifier};
            use ::$curve::ecdsa::signature::{Signer, Verifier};
            use ::$curve::ecdsa::{Signature, SigningKey, VerifyingKey};
            use ::$curve::pkcs8::EncodePublicKey;

            use super::Fail;

            pub(crate) fn public_key(secret: &[u8]) -> Option<Vec<u8>> {
                let key = SigningKey::from_slice(secret).ok()?;
                Some(
                    VerifyingKey::from(&key)
                        .to_public_key_der()
                        .ok()?
                        .as_bytes()
                        .to_vec(),
                )
            }

            /// Signs the message, or, with `digest`, the message as a digest.
            pub(crate) fn sign(secret: &[u8], message: &[u8], digest: bool) -> Option<Vec<u8>> {
                let key = SigningKey::from_slice(secret).ok()?;
                let sig: Signature = if digest {
                    key.sign_prehash(message).ok()?
                } else {
                    key.try_sign(message).ok()?
                };
                Some(sig.to_der().as_bytes().to_vec())
            }

            pub(crate) fn verify(
                secret: &[u8],
                message: &[u8],
                digest: bool,
                signature: &[u8],
            ) -> Result<(), Fail> {
                let key = SigningKey::from_slice(secret).map_err(|_| Fail::Malformed)?;
                let public = VerifyingKey::from(&key);
                let sig = Signature::from_der(signature).map_err(|_| Fail::Invalid)?;
                let ok = if digest {
                    public.verify_prehash(message, &sig)
                } else {
                    public.verify(message, &sig)
                };
                ok.map_err(|_| Fail::Invalid)
            }
        }
    };
}

ecdsa!(p256, Sha256);
ecdsa!(p384, Sha384);
ecdsa!(p521, Sha512);
ecdsa!(k256, Sha256);

/// The digest length each curve's hash makes, for a `DIGEST` message.
pub(crate) fn digest_len(spec: Spec) -> Option<usize> {
    Some(match spec {
        Spec::EccP256 | Spec::EccK1 => Sha256::output_size(),
        Spec::EccP384 => Sha384::output_size(),
        Spec::EccP521 | Spec::Ed25519 => Sha512::output_size(),
        _ => return None,
    })
}

/// The public key as a DER `SubjectPublicKeyInfo`.
pub(crate) fn public_key(spec: Spec, secret: &[u8]) -> Option<Vec<u8>> {
    match spec {
        Spec::EccP256 => p256::public_key(secret),
        Spec::EccP384 => p384::public_key(secret),
        Spec::EccP521 => p521::public_key(secret),
        Spec::EccK1 => k256::public_key(secret),
        Spec::Ed25519 => {
            let seed: [u8; 32] = secret.try_into().ok()?;
            let key = ed25519_dalek::SigningKey::from_bytes(&seed);
            // RFC 8410: the SubjectPublicKeyInfo of an Ed25519 key is this fixed prefix and the 32 bytes.
            let mut der = vec![
                0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
            ];
            der.extend_from_slice(key.verifying_key().as_bytes());
            Some(der)
        }
        _ => None,
    }
}

/// Signs. `digest` says `message` is already the digest.
pub(crate) fn sign(spec: Spec, secret: &[u8], message: &[u8], digest: bool) -> Option<Vec<u8>> {
    match spec {
        Spec::EccP256 => p256::sign(secret, message, digest),
        Spec::EccP384 => p384::sign(secret, message, digest),
        Spec::EccP521 => p521::sign(secret, message, digest),
        Spec::EccK1 => k256::sign(secret, message, digest),
        Spec::Ed25519 if !digest => {
            use ed25519_dalek::Signer;
            let seed: [u8; 32] = secret.try_into().ok()?;
            Some(
                ed25519_dalek::SigningKey::from_bytes(&seed)
                    .sign(message)
                    .to_bytes()
                    .to_vec(),
            )
        }
        _ => None,
    }
}

/// Verifies.
pub(crate) fn verify(
    spec: Spec,
    secret: &[u8],
    message: &[u8],
    digest: bool,
    signature: &[u8],
) -> Result<(), Fail> {
    match spec {
        Spec::EccP256 => p256::verify(secret, message, digest, signature),
        Spec::EccP384 => p384::verify(secret, message, digest, signature),
        Spec::EccP521 => p521::verify(secret, message, digest, signature),
        Spec::EccK1 => k256::verify(secret, message, digest, signature),
        Spec::Ed25519 if !digest => {
            use ed25519_dalek::Verifier;
            let seed: [u8; 32] = secret.try_into().map_err(|_| Fail::Malformed)?;
            let sig = ed25519_dalek::Signature::from_slice(signature).map_err(|_| Fail::Invalid)?;
            ed25519_dalek::SigningKey::from_bytes(&seed)
                .verifying_key()
                .verify(message, &sig)
                .map_err(|_| Fail::Invalid)
        }
        _ => Err(Fail::Malformed),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn every_curve_signs_and_verifies_and_refuses_a_change() {
        for spec in [
            Spec::EccP256,
            Spec::EccP384,
            Spec::EccP521,
            Spec::EccK1,
            Spec::Ed25519,
        ] {
            let secret = generate(spec).unwrap();
            let sig = sign(spec, &secret, b"msg", false).unwrap();
            assert_eq!(
                verify(spec, &secret, b"msg", false, &sig),
                Ok(()),
                "{spec:?}"
            );
            assert_eq!(
                verify(spec, &secret, b"other", false, &sig),
                Err(Fail::Invalid),
                "{spec:?}"
            );
            assert!(public_key(spec, &secret).unwrap().len() > 32, "{spec:?}");
        }
    }

    #[test]
    fn a_digest_is_signed_as_a_digest() {
        let secret = generate(Spec::EccP256).unwrap();
        let digest = Sha256::digest(b"msg");
        let sig = sign(Spec::EccP256, &secret, &digest, true).unwrap();
        // The signature over the digest is a signature over the message: the two ways agree.
        assert_eq!(verify(Spec::EccP256, &secret, b"msg", false, &sig), Ok(()));
        assert_eq!(verify(Spec::EccP256, &secret, &digest, true, &sig), Ok(()));
        assert_eq!(digest_len(Spec::EccP256), Some(32));
    }

    #[test]
    fn ed25519_matches_rfc_8032_test_1() {
        let seed = aws_core::hex::decode(
            "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
        )
        .unwrap();
        let sig = sign(Spec::Ed25519, &seed, b"", false).unwrap();
        assert_eq!(
            aws_core::hex::encode(&sig),
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
        );
        let public = public_key(Spec::Ed25519, &seed).unwrap();
        assert!(
            aws_core::hex::encode(&public)
                .ends_with("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
        );
    }

    #[test]
    fn a_garbled_signature_is_invalid_not_malformed() {
        let secret = generate(Spec::EccP384).unwrap();
        assert_eq!(
            verify(Spec::EccP384, &secret, b"m", false, b"junk"),
            Err(Fail::Invalid)
        );
    }
}
