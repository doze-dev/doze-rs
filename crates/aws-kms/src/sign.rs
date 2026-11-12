//! Signing and MACs: `Sign`, `Verify`, `GetPublicKey`, `GenerateMac`, `VerifyMac`.

use aws_model_kms::keys::{
    GenerateMacRequest as Mac, GenerateMacResponse as Maced, GetPublicKeyRequest as Public,
    GetPublicKeyResponse as PublicKey, SignRequest as Sign, SignResponse as Signed,
    VerifyMacRequest as VerifyMac, VerifyMacResponse as MacChecked, VerifyRequest as Verify,
    VerifyResponse as Verified,
};
use aws_wire::ApiError;
use aws_wire::handler::{reply, request};
use aws_wire::service::{Call, Reply};

use crate::asym::{self, Fail};
use crate::core::{Core, validation};
use crate::crypto;
use crate::find::usable;
use crate::model::{Key, State};
use crate::spec::Usage;

/// A message to sign: raw, or already a digest.
fn digest_flag(message_type: Option<&str>) -> bool {
    message_type == Some("DIGEST")
}

impl Core {
    /// The current material of `key`, opened.
    pub(crate) fn secret_of(&self, key: &Key) -> Result<Vec<u8>, ApiError> {
        let m = key
            .current()
            .ok_or_else(|| ApiError::internal("a key has no material"))?;
        self.sealer
            .open(&m.sealed)
            .ok_or_else(|| ApiError::internal("the key material could not be opened"))
    }

    /// The key, held to the algorithm: the key's spec must list it.
    fn signer<'a>(
        &'a self,
        call: &Call,
        id: &str,
        algorithm: &str,
        operation: &str,
    ) -> Result<&'a Key, ApiError> {
        let key = self.key(call, id)?;
        usable(key, Usage::SignVerify, operation)?;
        if !key.spec.signing_algorithms(key.usage).contains(&algorithm) {
            return Err(ApiError::modeled(
                "InvalidKeyUsageException",
                format!(
                    "Algorithm {algorithm} is incompatible with key spec {}.",
                    key.spec.name()
                ),
            ));
        }
        Ok(key)
    }

    pub(crate) fn sign(&mut self, call: &Call) -> Reply {
        let v = request::<Sign>(call)?;
        self.ensure_material(call, v.get(Sign::KEY_ID).unwrap_or_default())?;
        let algorithm = v.get(Sign::SIGNING_ALGORITHM).unwrap_or_default();
        let key = self.signer(
            call,
            v.get(Sign::KEY_ID).unwrap_or_default(),
            algorithm,
            "Sign",
        )?;
        let digest = digest_flag(v.get(Sign::MESSAGE_TYPE));
        let message = v.get(Sign::MESSAGE).unwrap_or_default();
        check_digest(key, algorithm, message, digest)?;
        let secret = self.secret_of(key)?;
        let signature =
            asym::sign(key.spec, algorithm, &secret, message, digest).ok_or_else(|| {
                ApiError::modeled(
                    "InvalidKeyUsageException",
                    "That message type is not valid for this algorithm.",
                )
            })?;
        reply::<Signed>(|o| {
            o.put(Signed::KEY_ID, &key.arn);
            o.put_blob(Signed::SIGNATURE, &signature);
            o.put(Signed::SIGNING_ALGORITHM, algorithm);
        })
    }

    pub(crate) fn verify(&mut self, call: &Call) -> Reply {
        let v = request::<Verify>(call)?;
        self.ensure_material(call, v.get(Verify::KEY_ID).unwrap_or_default())?;
        let algorithm = v.get(Verify::SIGNING_ALGORITHM).unwrap_or_default();
        let key = self.signer(
            call,
            v.get(Verify::KEY_ID).unwrap_or_default(),
            algorithm,
            "Verify",
        )?;
        let digest = digest_flag(v.get(Verify::MESSAGE_TYPE));
        let message = v.get(Verify::MESSAGE).unwrap_or_default();
        check_digest(key, algorithm, message, digest)?;
        let secret = self.secret_of(key)?;
        match asym::verify(
            key.spec,
            algorithm,
            &secret,
            message,
            digest,
            v.get(Verify::SIGNATURE).unwrap_or_default(),
        ) {
            Ok(()) => {}
            // Recorded 2026-11-09: a signature that does not match is an error with an empty message, not `false`.
            Err(Fail::Invalid) => {
                return Err(ApiError::modeled("KMSInvalidSignatureException", ""));
            }
            Err(Fail::Malformed) => {
                return Err(validation(
                    "The signature or the message is not in a form this key can check.",
                ));
            }
        }
        reply::<Verified>(|o| {
            o.put(Verified::KEY_ID, &key.arn);
            o.put_bool(Verified::SIGNATURE_VALID, true);
            o.put(Verified::SIGNING_ALGORITHM, algorithm);
        })
    }

    pub(crate) fn get_public_key(&mut self, call: &Call) -> Reply {
        let v = request::<Public>(call)?;
        self.ensure_material(call, v.get(Public::KEY_ID).unwrap_or_default())?;
        let key = self.key(call, v.get(Public::KEY_ID).unwrap_or_default())?;
        if key.state == State::PendingDeletion {
            return Err(crate::keys::invalid_state(format!(
                "{} is pending deletion.",
                key.arn
            )));
        }
        let der = asym::public_key(key.spec, &self.secret_of(key)?).ok_or_else(|| {
            ApiError::modeled(
                "UnsupportedOperationException",
                format!(
                    "{} is a symmetric or HMAC key: it has no public key.",
                    key.arn
                ),
            )
        })?;
        reply::<PublicKey>(|o| {
            o.put(PublicKey::KEY_ID, &key.arn);
            o.put_blob(PublicKey::PUBLIC_KEY, &der);
            o.put(PublicKey::CUSTOMER_MASTER_KEY_SPEC, key.spec.name());
            o.put(PublicKey::KEY_SPEC, key.spec.name());
            o.put(PublicKey::KEY_USAGE, key.usage.name());
            o.put_list(PublicKey::SIGNING_ALGORITHMS, |l| {
                for a in key.spec.signing_algorithms(key.usage) {
                    l.push(a);
                }
            });
        })
    }

    /// The key, held to a MAC algorithm.
    fn macer<'a>(
        &'a self,
        call: &Call,
        id: &str,
        algorithm: &str,
        operation: &str,
    ) -> Result<&'a Key, ApiError> {
        let key = self.key(call, id)?;
        usable(key, Usage::GenerateVerifyMac, operation)?;
        if !key.spec.mac_algorithms().contains(&algorithm) {
            return Err(ApiError::modeled(
                "InvalidKeyUsageException",
                format!(
                    "Algorithm {algorithm} is incompatible with key spec {}.",
                    key.spec.name()
                ),
            ));
        }
        Ok(key)
    }

    pub(crate) fn generate_mac(&mut self, call: &Call) -> Reply {
        let v = request::<Mac>(call)?;
        let algorithm = v.get(Mac::MAC_ALGORITHM).unwrap_or_default();
        let key = self.macer(
            call,
            v.get(Mac::KEY_ID).unwrap_or_default(),
            algorithm,
            "GenerateMac",
        )?;
        let tag = crypto::mac(
            algorithm,
            &self.secret_of(key)?,
            v.get(Mac::MESSAGE).unwrap_or_default(),
        )
        .ok_or_else(|| validation("That MAC algorithm is not supported."))?;
        reply::<Maced>(|o| {
            o.put_blob(Maced::MAC, &tag);
            o.put(Maced::MAC_ALGORITHM, algorithm);
            o.put(Maced::KEY_ID, &key.arn);
        })
    }

    pub(crate) fn verify_mac(&mut self, call: &Call) -> Reply {
        let v = request::<VerifyMac>(call)?;
        let algorithm = v.get(VerifyMac::MAC_ALGORITHM).unwrap_or_default();
        let key = self.macer(
            call,
            v.get(VerifyMac::KEY_ID).unwrap_or_default(),
            algorithm,
            "VerifyMac",
        )?;
        let ok = crypto::mac_valid(
            algorithm,
            &self.secret_of(key)?,
            v.get(VerifyMac::MESSAGE).unwrap_or_default(),
            v.get(VerifyMac::MAC).unwrap_or_default(),
        );
        if !ok {
            return Err(ApiError::modeled("KMSInvalidMacException", ""));
        }
        reply::<MacChecked>(|o| {
            o.put(MacChecked::KEY_ID, &key.arn);
            o.put_bool(MacChecked::MAC_VALID, true);
            o.put(MacChecked::MAC_ALGORITHM, algorithm);
        })
    }
}

/// A digest must be as long as the digest the key's algorithm makes.
fn check_digest(key: &Key, algorithm: &str, message: &[u8], digest: bool) -> Result<(), ApiError> {
    if digest && asym::digest_len(key.spec, algorithm) != Some(message.len()) {
        return Err(validation(
            "The message is not the length of a digest for this algorithm.",
        ));
    }
    Ok(())
}
