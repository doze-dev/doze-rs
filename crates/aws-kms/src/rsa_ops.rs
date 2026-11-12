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

fn algorithm_of(key: &Key, given: Option<&str>) -> Result<String, ApiError> {
    let allowed = key.spec.encryption_algorithms(key.usage);
    match given {
        Some(a) if allowed.contains(&a) => Ok(a.to_owned()),
        Some(a) => Err(ApiError::modeled(
            "InvalidKeyUsageException",
            format!(
                "Algorithm {a} is incompatible with key spec {}.",
                key.spec.name()
            ),
        )),
        None => Err(validation("An RSA key needs an EncryptionAlgorithm.")),
    }
}

impl Core {
    /// Makes the material of an RSA key the first time it is wanted. Any other key already has its material.
    pub(crate) fn ensure_material(&mut self, call: &Call, given: &str) -> Result<(), ApiError> {
        let id = self.locate(call, given)?;
        let key = self.keys.get(&id).ok_or_else(|| not_found_key(call, &id))?;
        let Some(bits) = key.spec.rsa_bits() else {
            return Ok(());
        };
        if key.current().is_none_or(|m| !m.sealed.is_empty()) {
            return Ok(());
        }
        let der = rsa_keys::generate(bits).ok_or_else(no_randomness)?;
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
        let der = self.secret_of(key)?;
        let blob = rsa_keys::encrypt(
            &algorithm,
            &der,
            v.get(Encrypt::PLAINTEXT).unwrap_or_default(),
        )
        .ok_or_else(|| validation("The plaintext is too long for this key and algorithm."))?;
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
        // Recorded for a symmetric key: the message of an unreadable ciphertext is empty.
        let plain = rsa_keys::decrypt(
            &algorithm,
            &der,
            v.get(Decrypt::CIPHERTEXT_BLOB).unwrap_or_default(),
        )
        .ok_or_else(|| ApiError::modeled("InvalidCiphertextException", ""))?;
        reply::<Decrypted>(|o| {
            o.put(Decrypted::KEY_ID, &key.arn);
            o.put_blob(Decrypted::PLAINTEXT, &plain);
            o.put(Decrypted::ENCRYPTION_ALGORITHM, &algorithm);
        })
    }
}
