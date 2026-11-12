//! Keeping a value under a KMS key the way AWS services do: envelope encryption. KMS makes a data key (`GenerateDataKey`) bound to an
//! encryption context; the value is sealed with the data key; what is stored is the KMS ciphertext of the data key beside the sealed
//! value. Opening asks KMS to `Decrypt` the data key (so a key that is disabled, scheduled for deletion or gone stops the value from
//! opening, and the context must be the same), then opens the value. There is no limit to the value's size but memory.
//!
//! The stored bytes: `0x01 | length of the KMS blob (u32, big-endian) | the KMS blob | AES-256-GCM sealed value`.

use aws_seal::Sealer;
use aws_wire::ApiError;
use aws_wire::service::Caller;
use serde_json::{Value, json};

use crate::Peers;

const VERSION: u8 = 1;

fn bad(what: &str) -> ApiError {
    ApiError::internal(format!("KMS answered without {what}"))
}

/// Seals `plain` under `key_id`, with `context` (a JSON object of strings), as service `acting` for `caller`.
///
/// # Errors
/// KMS's own error (a key that does not exist, is disabled, or is pending deletion), or an internal error.
pub async fn seal(
    peers: &Peers,
    acting: &str,
    caller: &Caller,
    key_id: &str,
    context: &Value,
    plain: &[u8],
) -> Result<Vec<u8>, ApiError> {
    let made = peers
        .call(
            acting,
            caller,
            "kms",
            "GenerateDataKey",
            &json!({"KeyId": key_id, "KeySpec": "AES_256", "EncryptionContext": context}),
        )
        .await?;
    let key = made
        .get("Plaintext")
        .and_then(Value::as_str)
        .and_then(aws_core::base64::decode)
        .ok_or_else(|| bad("a data key"))?;
    let blob = made
        .get("CiphertextBlob")
        .and_then(Value::as_str)
        .and_then(aws_core::base64::decode)
        .ok_or_else(|| bad("a ciphertext"))?;
    let sealed = Sealer::new(&key)
        .and_then(|s| s.seal(plain))
        .ok_or_else(|| ApiError::internal("the value could not be sealed"))?;
    let len = u32::try_from(blob.len())
        .map_err(|_| ApiError::internal("a KMS ciphertext is too long"))?;
    let mut out = vec![VERSION];
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&blob);
    out.extend_from_slice(&sealed);
    Ok(out)
}

/// Opens what [`seal`] made, with the same `context`.
///
/// # Errors
/// KMS's own error, or an internal error if the bytes are not what [`seal`] makes.
pub async fn open(
    peers: &Peers,
    acting: &str,
    caller: &Caller,
    context: &Value,
    stored: &[u8],
) -> Result<Vec<u8>, ApiError> {
    let corrupt = || ApiError::internal("a stored value could not be opened");
    let (&version, rest) = stored.split_first().ok_or_else(corrupt)?;
    let (len, rest) = rest.split_at_checked(4).ok_or_else(corrupt)?;
    let len = usize::try_from(u32::from_be_bytes(len.try_into().map_err(|_| corrupt())?))
        .map_err(|_| corrupt())?;
    let (blob, sealed) = rest.split_at_checked(len).ok_or_else(corrupt)?;
    if version != VERSION {
        return Err(corrupt());
    }
    let opened = peers
        .call(acting, caller, "kms", "Decrypt", &json!({"CiphertextBlob": aws_core::base64::encode(blob), "EncryptionContext": context}))
        .await?;
    let key = opened
        .get("Plaintext")
        .and_then(Value::as_str)
        .and_then(aws_core::base64::decode)
        .ok_or_else(|| bad("a data key"))?;
    Sealer::new(&key)
        .and_then(|s| s.open(sealed))
        .ok_or_else(corrupt)
}
