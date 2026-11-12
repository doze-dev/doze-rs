//! Encrypting: `Encrypt`, `Decrypt`, `ReEncrypt`, `GenerateDataKey`, `GenerateDataKeyWithoutPlaintext`, `GenerateRandom`.

use std::collections::BTreeMap;

use aws_model_kms::keys::{
    DecryptRequest as Decrypt, DecryptResponse as Decrypted, EncryptRequest as Encrypt,
    EncryptResponse as Encrypted, GenerateDataKeyRequest as DataKey,
    GenerateDataKeyResponse as DataKeyMade,
    GenerateDataKeyWithoutPlaintextRequest as DataKeySealed,
    GenerateDataKeyWithoutPlaintextResponse as DataKeySealedMade, GenerateRandomRequest as Random,
    GenerateRandomResponse as RandomMade, ReEncryptRequest as ReEncrypt,
    ReEncryptResponse as ReEncrypted,
};
use aws_wire::ApiError;
use aws_wire::handler::{reply, request};
use aws_wire::service::{Call, Reply};

use crate::core::{Core, validation};
use crate::crypto;
use crate::find::{not_found_key, usable};
use crate::keys::no_randomness;
use crate::model::Key;
use crate::spec::Usage;

const SYMMETRIC: &str = "SYMMETRIC_DEFAULT";
const MAX_RANDOM: usize = 1024;

/// The encryption context of a request.
fn context<'a>(
    pairs: Option<impl Iterator<Item = (&'a str, &'a str)>>,
) -> BTreeMap<String, String> {
    pairs
        .into_iter()
        .flatten()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

fn invalid_ciphertext() -> ApiError {
    // Recorded 2026-11-09: the message is empty.
    ApiError::modeled("InvalidCiphertextException", "")
}

/// What opening a blob found.
struct Opened {
    key: Key,
    plain: Vec<u8>,
    material_id: String,
}

impl Core {
    /// The current material of `key`, opened.
    fn raw_current(&self, key: &Key) -> Result<Vec<u8>, ApiError> {
        let m = key
            .current()
            .ok_or_else(|| ApiError::internal("a key has no material"))?;
        self.sealer
            .open(&m.sealed)
            .ok_or_else(|| ApiError::internal("the key material could not be opened"))
    }

    /// Opens a ciphertext blob. When the request names a key it must be the blob's.
    fn open_blob(
        &self,
        call: &Call,
        blob: &[u8],
        ctx: &BTreeMap<String, String>,
        given: Option<&str>,
    ) -> Result<Opened, ApiError> {
        let parsed = crypto::parse(blob).ok_or_else(invalid_ciphertext)?;
        let key = self
            .keys
            .get(parsed.key_id)
            .ok_or_else(|| not_found_key(call, parsed.key_id))?;
        if let Some(given) = given
            && self.locate(call, given)? != key.id
        {
            return Err(ApiError::modeled(
                "IncorrectKeyException",
                "The key ID in the request does not identify a CMK that can perform this operation.",
            ));
        }
        usable(key, Usage::EncryptDecrypt, "Decrypt")?;
        let material_id = aws_core::hex::encode(&parsed.material);
        let material = key.material(&material_id).ok_or_else(invalid_ciphertext)?;
        let raw = self
            .sealer
            .open(&material.sealed)
            .ok_or_else(|| ApiError::internal("the key material could not be opened"))?;
        let plain = crypto::open(&parsed, &raw, ctx).ok_or_else(invalid_ciphertext)?;
        Ok(Opened {
            key: key.clone(),
            plain,
            material_id,
        })
    }

    /// Seals `plain` under the current material of the key `given` names; returns the blob, the key, and the material's id.
    fn seal_under(
        &self,
        call: &Call,
        given: &str,
        ctx: &BTreeMap<String, String>,
        plain: &[u8],
        operation: &str,
    ) -> Result<(Vec<u8>, Key, String), ApiError> {
        let key = self.key(call, given)?;
        usable(key, Usage::EncryptDecrypt, operation)?;
        let raw = self.raw_current(key)?;
        let blob = crypto::seal(&key.id, &raw, ctx, plain).ok_or_else(no_randomness)?;
        let material = key.current().map(|m| m.id.clone()).unwrap_or_default();
        Ok((blob, key.clone(), material))
    }

    pub(crate) fn encrypt(&mut self, call: &Call) -> Reply {
        let v = request::<Encrypt>(call)?;
        if v.get(Encrypt::ENCRYPTION_ALGORITHM)
            .is_some_and(|a| a != SYMMETRIC)
        {
            return Err(ApiError::modeled(
                "InvalidKeyUsageException",
                "The encryption algorithm is not valid for this key.",
            ));
        }
        let ctx = context(v.get(Encrypt::ENCRYPTION_CONTEXT));
        let (blob, key, _) = self.seal_under(
            call,
            v.get(Encrypt::KEY_ID).unwrap_or_default(),
            &ctx,
            v.get(Encrypt::PLAINTEXT).unwrap_or_default(),
            "Encrypt",
        )?;
        reply::<Encrypted>(|o| {
            o.put_blob(Encrypted::CIPHERTEXT_BLOB, &blob);
            o.put(Encrypted::KEY_ID, &key.arn);
            o.put(Encrypted::ENCRYPTION_ALGORITHM, SYMMETRIC);
        })
    }

    pub(crate) fn decrypt(&mut self, call: &Call) -> Reply {
        let v = request::<Decrypt>(call)?;
        let ctx = context(v.get(Decrypt::ENCRYPTION_CONTEXT));
        let opened = self.open_blob(
            call,
            v.get(Decrypt::CIPHERTEXT_BLOB).unwrap_or_default(),
            &ctx,
            v.get(Decrypt::KEY_ID),
        )?;
        reply::<Decrypted>(|o| {
            o.put(Decrypted::KEY_ID, &opened.key.arn);
            o.put_blob(Decrypted::PLAINTEXT, &opened.plain);
            o.put(Decrypted::ENCRYPTION_ALGORITHM, SYMMETRIC);
            o.put(Decrypted::KEY_MATERIAL_ID, &opened.material_id);
        })
    }

    pub(crate) fn re_encrypt(&mut self, call: &Call) -> Reply {
        let v = request::<ReEncrypt>(call)?;
        let from = context(v.get(ReEncrypt::SOURCE_ENCRYPTION_CONTEXT));
        let to = context(v.get(ReEncrypt::DESTINATION_ENCRYPTION_CONTEXT));
        let opened = self.open_blob(
            call,
            v.get(ReEncrypt::CIPHERTEXT_BLOB).unwrap_or_default(),
            &from,
            v.get(ReEncrypt::SOURCE_KEY_ID),
        )?;
        let (blob, dest, dest_material) = self.seal_under(
            call,
            v.get(ReEncrypt::DESTINATION_KEY_ID).unwrap_or_default(),
            &to,
            &opened.plain,
            "ReEncrypt",
        )?;
        reply::<ReEncrypted>(|o| {
            o.put_blob(ReEncrypted::CIPHERTEXT_BLOB, &blob);
            o.put(ReEncrypted::SOURCE_KEY_ID, &opened.key.arn);
            o.put(ReEncrypted::KEY_ID, &dest.arn);
            o.put(ReEncrypted::SOURCE_ENCRYPTION_ALGORITHM, SYMMETRIC);
            o.put(ReEncrypted::DESTINATION_ENCRYPTION_ALGORITHM, SYMMETRIC);
            o.put(ReEncrypted::SOURCE_KEY_MATERIAL_ID, &opened.material_id);
            o.put(ReEncrypted::DESTINATION_KEY_MATERIAL_ID, &dest_material);
        })
    }

    pub(crate) fn generate_data_key(&mut self, call: &Call) -> Reply {
        let v = request::<DataKey>(call)?;
        let len = data_key_len(v.get(DataKey::KEY_SPEC), v.get(DataKey::NUMBER_OF_BYTES))?;
        let plain = random(len)?;
        let ctx = context(v.get(DataKey::ENCRYPTION_CONTEXT));
        let (blob, key, material) = self.seal_under(
            call,
            v.get(DataKey::KEY_ID).unwrap_or_default(),
            &ctx,
            &plain,
            "GenerateDataKey",
        )?;
        reply::<DataKeyMade>(|o| {
            o.put_blob(DataKeyMade::CIPHERTEXT_BLOB, &blob);
            o.put_blob(DataKeyMade::PLAINTEXT, &plain);
            o.put(DataKeyMade::KEY_ID, &key.arn);
            o.put(DataKeyMade::KEY_MATERIAL_ID, &material);
        })
    }

    pub(crate) fn generate_data_key_without_plaintext(&mut self, call: &Call) -> Reply {
        let v = request::<DataKeySealed>(call)?;
        let len = data_key_len(
            v.get(DataKeySealed::KEY_SPEC),
            v.get(DataKeySealed::NUMBER_OF_BYTES),
        )?;
        let plain = random(len)?;
        let ctx = context(v.get(DataKeySealed::ENCRYPTION_CONTEXT));
        let (blob, key, material) = self.seal_under(
            call,
            v.get(DataKeySealed::KEY_ID).unwrap_or_default(),
            &ctx,
            &plain,
            "GenerateDataKeyWithoutPlaintext",
        )?;
        reply::<DataKeySealedMade>(|o| {
            o.put_blob(DataKeySealedMade::CIPHERTEXT_BLOB, &blob);
            o.put(DataKeySealedMade::KEY_ID, &key.arn);
            o.put(DataKeySealedMade::KEY_MATERIAL_ID, &material);
        })
    }

    pub(crate) fn generate_random(&mut self, call: &Call) -> Reply {
        let v = request::<Random>(call)?;
        if v.get(Random::RECIPIENT).is_some() || v.get(Random::CUSTOM_KEY_STORE_ID).is_some() {
            return Err(ApiError::modeled(
                "UnsupportedOperationException",
                "Recipient and custom key stores are not built in doze-aws.",
            ));
        }
        let n = usize::try_from(v.get(Random::NUMBER_OF_BYTES).unwrap_or(32)).unwrap_or(32);
        if !(1..=MAX_RANDOM).contains(&n) {
            return Err(validation("NumberOfBytes must be between 1 and 1024"));
        }
        let bytes = random(n)?;
        reply::<RandomMade>(|o| {
            o.put_blob(RandomMade::PLAINTEXT, &bytes);
        })
    }
}

/// How long a data key is: its `KeySpec`, or `NumberOfBytes`, not both.
fn data_key_len(spec: Option<&str>, bytes: Option<i64>) -> Result<usize, ApiError> {
    match (spec, bytes) {
        (Some(_), Some(_)) => Err(validation(
            "Please specify either number of bytes or key spec.",
        )),
        (Some("AES_256"), None) => Ok(32),
        (Some("AES_128"), None) => Ok(16),
        (Some(other), None) => Err(validation(format!(
            "The key spec {other} is not valid for a data key."
        ))),
        (None, Some(n)) if (1..=1024).contains(&n) => Ok(usize::try_from(n).unwrap_or(32)),
        (None, Some(_)) => Err(validation("NumberOfBytes must be between 1 and 1024")),
        (None, None) => Err(validation(
            "Please specify either number of bytes or key spec.",
        )),
    }
}

fn random(n: usize) -> Result<Vec<u8>, ApiError> {
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        out.extend_from_slice(&aws_seal::random_bytes::<32>().ok_or_else(no_randomness)?);
    }
    out.truncate(n);
    Ok(out)
}
