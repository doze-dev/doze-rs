//! Key agreement and last use: `DeriveSharedSecret` (ECDH between a `KEY_AGREEMENT` key on a NIST curve and a peer's public key,
//! answering the raw shared secret: the x-coordinate of the shared point, as the model's documentation says) and `GetKeyLastUsage`
//! (the last successful cryptographic operation of a key).
//!
//! Last use is kept in memory, not on disk: `TrackingStartDate` is when this region's KMS last started (or the key was made, if
//! later), so after a restart a key reads as not used since tracking began, as a key does on AWS before tracking started. No
//! CloudTrail runs here, so `CloudTrailEventId` is never given; `KmsRequestId` is the request's id.

use aws_model_kms::Operation;
use aws_model_kms::keys::{
    DeriveSharedSecretRequest as Derive, DeriveSharedSecretResponse as Derived,
    GetKeyLastUsageRequest as LastUsage, GetKeyLastUsageResponse as LastUsed, KeyLastUsageData,
};
use aws_wire::ApiError;
use aws_wire::handler::{reply, request};
use aws_wire::service::{Call, Reply};
use serde_json::Value;

use crate::core::{Core, validation};
use crate::find::usable;
use crate::keys::seconds;
use crate::spec::{Spec, Usage};

/// A key's last successful cryptographic operation.
#[derive(Clone, Debug)]
pub(crate) struct Used {
    pub(crate) operation: &'static str,
    pub(crate) at_ms: i64,
    pub(crate) request_id: String,
}

macro_rules! ecdh {
    ($curve:ident) => {
        fn $curve(secret: &[u8], peer: &[u8]) -> Option<Vec<u8>> {
            use ::$curve::pkcs8::DecodePublicKey;
            let secret = ::$curve::SecretKey::from_slice(secret).ok()?;
            let public = ::$curve::PublicKey::from_public_key_der(peer).ok()?;
            let shared =
                ::$curve::ecdh::diffie_hellman(secret.to_nonzero_scalar(), public.as_affine());
            Some(shared.raw_secret_bytes().to_vec())
        }
    };
}

ecdh!(p256);
ecdh!(p384);
ecdh!(p521);

/// The shared secret of our `secret` on `spec` and a peer's SPKI public key; `None` when the peer's key is not on the same curve.
pub(crate) fn shared_secret(spec: Spec, secret: &[u8], peer: &[u8]) -> Option<Vec<u8>> {
    match spec {
        Spec::EccP256 => p256(secret, peer),
        Spec::EccP384 => p384(secret, peer),
        Spec::EccP521 => p521(secret, peer),
        _ => None,
    }
}

/// The operations whose success counts as a use of the key they name.
fn tracked(op: Operation) -> Option<&'static str> {
    use Operation as O;
    Some(match op {
        O::Decrypt => "Decrypt",
        O::DeriveSharedSecret => "DeriveSharedSecret",
        O::Encrypt => "Encrypt",
        O::GenerateDataKey => "GenerateDataKey",
        O::GenerateDataKeyPair => "GenerateDataKeyPair",
        O::GenerateDataKeyPairWithoutPlaintext => "GenerateDataKeyPairWithoutPlaintext",
        O::GenerateDataKeyWithoutPlaintext => "GenerateDataKeyWithoutPlaintext",
        O::GenerateMac => "GenerateMac",
        O::ReEncrypt => "ReEncrypt",
        O::Sign => "Sign",
        O::Verify => "Verify",
        O::VerifyMac => "VerifyMac",
        _ => return None,
    })
}

impl Core {
    pub(crate) fn derive_shared_secret(&mut self, call: &Call) -> Reply {
        let v = request::<Derive>(call)?;
        let key = self.key(call, v.get(Derive::KEY_ID).unwrap_or_default())?;
        usable(key, Usage::KeyAgreement, "DeriveSharedSecret")?;
        if v.get(Derive::RECIPIENT).is_some() {
            return Err(ApiError::modeled(
                "UnsupportedOperationException",
                "doze-aws has no Nitro Enclaves to attest: call DeriveSharedSecret without Recipient.",
            ));
        }
        let peer = v.get(Derive::PUBLIC_KEY).unwrap_or_default();
        let secret = shared_secret(key.spec, &self.secret_of(key)?, peer).ok_or_else(|| {
            // (memory)
            validation(format!(
                "The PublicKey is not a DER-encoded SubjectPublicKeyInfo of a {} key.",
                key.spec.name()
            ))
        })?;
        if v.get(Derive::DRY_RUN) == Some(true) {
            // (memory)
            return Err(ApiError::modeled(
                "DryRunOperationException",
                "The request would have succeeded, but the DryRun option is set.",
            ));
        }
        let origin = if key.external { "EXTERNAL" } else { "AWS_KMS" };
        reply::<Derived>(|o| {
            o.put(Derived::KEY_ID, &key.arn);
            o.put_blob(Derived::SHARED_SECRET, &secret);
            o.put(Derived::KEY_AGREEMENT_ALGORITHM, "ECDH");
            o.put(Derived::KEY_ORIGIN, origin);
        })
    }

    pub(crate) fn get_key_last_usage(&mut self, call: &Call) -> Reply {
        let v = request::<LastUsage>(call)?;
        let given = v.get(LastUsage::KEY_ID).unwrap_or_default();
        if given.starts_with("alias/") || given.contains(":alias/") {
            // The model: alias names are not supported here. (memory: the words)
            return Err(validation(
                "GetKeyLastUsage takes a key ID or key ARN, not an alias.",
            ));
        }
        let key = self.key(call, given)?;
        let used = self.used.get(&key.id).cloned();
        let start = self.started_ms.max(key.created_ms);
        reply::<LastUsed>(|o| {
            o.put(LastUsed::KEY_ID, &key.id);
            o.put_time(LastUsed::TRACKING_START_DATE, seconds(start));
            o.put_time(LastUsed::KEY_CREATION_DATE, seconds(key.created_ms));
            o.put_object(LastUsed::KEY_LAST_USAGE, |d| {
                if let Some(u) = &used {
                    d.put(KeyLastUsageData::OPERATION, u.operation);
                    d.put_time(KeyLastUsageData::TIMESTAMP, seconds(u.at_ms));
                    d.put(KeyLastUsageData::KMS_REQUEST_ID, &u.request_id);
                }
            });
        })
    }

    /// Notes a successful cryptographic call as the last use of the key it named.
    pub(crate) fn note_use(&mut self, call: &Call) {
        let Some(operation) = Operation::from_index(call.op).and_then(tracked) else {
            return;
        };
        let body: Value = serde_json::from_str(&call.body).unwrap_or(Value::Null);
        let text = |k: &str| body.get(k).and_then(Value::as_str);
        let given = text("DestinationKeyId").or_else(|| text("KeyId"));
        let id = match given {
            Some(g) => self.locate(call, g).ok(),
            None => text("CiphertextBlob")
                .and_then(aws_core::base64::decode)
                .and_then(|b| crate::crypto::parse(&b).map(|p| p.key_id.to_owned())),
        };
        if let Some(id) = id {
            let used = Used {
                operation,
                at_ms: self.now_ms(),
                request_id: call.id.to_string(),
            };
            self.used.insert(id, used);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn both_sides_derive_the_same_secret() {
        for spec in [Spec::EccP256, Spec::EccP384, Spec::EccP521] {
            let a = crate::asym::generate(spec).unwrap();
            let b = crate::asym::generate(spec).unwrap();
            let (pa, pb) = (
                crate::asym::public_key(spec, &a).unwrap(),
                crate::asym::public_key(spec, &b).unwrap(),
            );
            let ab = shared_secret(spec, &a, &pb).unwrap();
            assert_eq!(ab, shared_secret(spec, &b, &pa).unwrap(), "{spec:?}");
            assert_eq!(ab.len(), spec.material_len());
        }
        let p256 = crate::asym::generate(Spec::EccP256).unwrap();
        let p384 = crate::asym::public_key(
            Spec::EccP384,
            &crate::asym::generate(Spec::EccP384).unwrap(),
        )
        .unwrap();
        assert_eq!(shared_secret(Spec::EccP256, &p256, &p384), None);
    }
}
