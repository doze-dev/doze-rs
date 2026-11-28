//! RSA keys at work in `Encrypt` and `Decrypt` (OAEP), and the lazy making of their material. An RSA key is cheap to create and
//! costs its generation (about half a second at 4,096 bits in a release build) the first time it is used.

use aws_model_kms::keys::{
    DecryptRequest as Decrypt, DecryptResponse as Decrypted, EncryptRequest as Encrypt,
    EncryptResponse as Encrypted,
};
use aws_wire::ApiError;
use aws_wire::handler::{reply, request};
use aws_wire::service::{Call, Reply};

use crate::core::{Core, validation};
use crate::find::{not_found_key, usable};
use crate::keys::no_randomness;
use crate::model::Key;
use crate::rsa_keys;
use crate::spec::Usage;

/// The algorithm a call names, which an RSA key must support; none named is `SYMMETRIC_DEFAULT`, which it never does (AWS's
/// words for both, recorded 2026-11-15).
fn algorithm_of(key: &Key, given: Option<&str>) -> Result<String, ApiError> {
    let a = given.unwrap_or(crate::cipher::SYMMETRIC);
    if key.spec.encryption_algorithms(key.usage).contains(&a) {
        return Ok(a.to_owned());
    }
    Err(ApiError::modeled(
        "InvalidKeyUsageException",
        format!(
            "Algorithm {a} is incompatible with key spec {}.",
            key.spec.name()
        ),
    ))
}

/// The most bytes OAEP seals under a key of `bits`: the modulus less two digests and two bytes.
fn oaep_limit(bits: usize, algorithm: &str) -> usize {
    let digest = if algorithm == "RSAES_OAEP_SHA_1" {
        20
    } else {
        32
    };
    (bits / 8).saturating_sub(2 * digest + 2)
}

impl Core {
    /// Makes the material of an RSA key the first time it is wanted. Any other key already has its material.
    pub(crate) fn ensure_material(&mut self, call: &Call, given: &str) -> Result<(), ApiError> {
        self.ensure_managed(call, given)?;
        let id = self.locate(call, given)?;
        let key = self.keys.get(&id).ok_or_else(|| not_found_key(call, &id))?;
        let Some(bits) = key.spec.rsa_bits() else {
            return Ok(());
        };
        if key.current().is_none_or(|m| !m.sealed.is_empty()) {
            return Ok(());
        }
        // Made off the event loop: the call is asked again with it (`offload.rs`).
        let (der, _) = self.fresh_rsa(bits)?;
        let sealed = self.sealer.seal(&der).ok_or_else(no_randomness)?;
        if let Some(m) = self.keys.get_mut(&id).and_then(|k| k.materials.last_mut()) {
            m.sealed = sealed;
        }
        Ok(())
    }

    /// Whether the key `given` names is an RSA key.
    pub(crate) fn is_rsa(&self, call: &Call, given: &str) -> Result<bool, ApiError> {
        Ok(self.key(call, given)?.spec.is_rsa())
    }

    pub(crate) fn rsa_encrypt(&mut self, call: &Call) -> Reply {
        let v = request::<Encrypt>(call)?;
        let key = self.key(call, v.get(Encrypt::KEY_ID).unwrap_or_default())?;
        usable(key, Usage::EncryptDecrypt, "Encrypt")?;
        let algorithm = algorithm_of(key, v.get(Encrypt::ENCRYPTION_ALGORITHM))?;
        if v.get(Encrypt::ENCRYPTION_CONTEXT)
            .is_some_and(|mut c| c.next().is_some())
        {
            return Err(validation(
                "An encryption context is not supported with an asymmetric key.",
            ));
        }
        let plain = v.get(Encrypt::PLAINTEXT).unwrap_or_default();
        let limit = oaep_limit(key.spec.rsa_bits().unwrap_or(0), &algorithm);
        if plain.len() > limit {
            return Err(validation(format!(
                "Algorithm {algorithm} and key spec {} cannot encrypt data larger than {limit} bytes.",
                key.spec.name()
            )));
        }
        let der = self.secret_of(key)?;
        let blob = rsa_keys::encrypt(&algorithm, &der, plain).ok_or_else(no_randomness)?;
        reply::<Encrypted>(|o| {
            o.put_blob(Encrypted::CIPHERTEXT_BLOB, &blob);
            o.put(Encrypted::KEY_ID, &key.arn);
            o.put(Encrypted::ENCRYPTION_ALGORITHM, &algorithm);
        })
    }

    pub(crate) fn rsa_decrypt(&mut self, call: &Call) -> Reply {
        let v = request::<Decrypt>(call)?;
        let key = self.key(call, v.get(Decrypt::KEY_ID).unwrap_or_default())?;
        usable(key, Usage::EncryptDecrypt, "Decrypt")?;
        let algorithm = algorithm_of(key, v.get(Decrypt::ENCRYPTION_ALGORITHM))?;
        let der = self.secret_of(key)?;
        let arn = key.arn.clone();
        let blob = v.get(Decrypt::CIPHERTEXT_BLOB).unwrap_or_default().to_vec();
        // A private-key operation: about 12 ms at 4,096 bits, made off the event loop (`offload.rs`).
        self.defer(call, move || {
            // Recorded for a symmetric key: the message of an unreadable ciphertext is empty.
            let plain = rsa_keys::decrypt(&algorithm, &der, &blob)
                .ok_or_else(|| ApiError::modeled("InvalidCiphertextException", ""))?;
            reply::<Decrypted>(|o| {
                o.put(Decrypted::KEY_ID, &arn);
                o.put_blob(Decrypted::PLAINTEXT, &plain);
                o.put(Decrypted::ENCRYPTION_ALGORITHM, &algorithm);
            })
        })
    }
}
