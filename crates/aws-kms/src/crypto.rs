//! The symmetric half of KMS: the ciphertext blob, and MACs.
//!
//! A blob is `0x01 | len(key id) | key id | material id (32 bytes) | nonce (12) | AES-256-GCM(plaintext)`. The key id and the material
//! id travel in the blob so a `Decrypt` without a `KeyId` finds the key, and so a rotated key still opens what an older version of
//! it sealed. The encryption context is the associated data: leave it out, or change it, and the tag does not verify.

use std::collections::BTreeMap;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha224, Sha256, Sha384, Sha512};

const VERSION: u8 = 1;
const NONCE: usize = 12;
/// The size of a key's material id, a SHA-256 of the material.
pub(crate) const MATERIAL_ID: usize = 32;

/// The encryption context as the bytes the tag covers: pairs in key order, each part ended by a zero.
fn aad(context: &BTreeMap<String, String>) -> Vec<u8> {
    let mut out = Vec::new();
    for (k, v) in context {
        out.extend_from_slice(k.as_bytes());
        out.push(0);
        out.extend_from_slice(v.as_bytes());
        out.push(0);
    }
    out
}

/// The id of key material: its SHA-256.
pub(crate) fn material_id(material: &[u8]) -> [u8; MATERIAL_ID] {
    Sha256::digest(material).into()
}

/// A ciphertext blob taken apart.
pub(crate) struct Blob<'a> {
    pub(crate) key_id: &'a str,
    pub(crate) material: [u8; MATERIAL_ID],
    nonce: [u8; NONCE],
    body: &'a [u8],
}

/// Seals `plain` under `material` (32 bytes) for key `key_id`.
pub(crate) fn seal(
    key_id: &str,
    material: &[u8],
    context: &BTreeMap<String, String>,
    plain: &[u8],
) -> Option<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(material).ok()?;
    let nonce = aws_seal::random_bytes::<NONCE>()?;
    let body = cipher
        .encrypt(
            &Nonce::from(nonce),
            Payload {
                msg: plain,
                aad: &aad(context),
            },
        )
        .ok()?;
    let mut out = vec![VERSION, u8::try_from(key_id.len()).ok()?];
    out.extend_from_slice(key_id.as_bytes());
    out.extend_from_slice(&material_id(material));
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&body);
    Some(out)
}

/// Takes a blob apart; `None` when it is not one of ours.
pub(crate) fn parse(blob: &[u8]) -> Option<Blob<'_>> {
    let (&version, rest) = blob.split_first()?;
    let (&id_len, rest) = rest.split_first()?;
    if version != VERSION {
        return None;
    }
    let (id, rest) = rest.split_at_checked(usize::from(id_len))?;
    let (material, rest) = rest.split_at_checked(MATERIAL_ID)?;
    let (nonce, body) = rest.split_at_checked(NONCE)?;
    Some(Blob {
        key_id: std::str::from_utf8(id).ok()?,
        material: material.try_into().ok()?,
        nonce: nonce.try_into().ok()?,
        body,
    })
}

/// Opens a blob with `material`; `None` when the tag does not verify (a wrong key, a changed context, a changed blob).
pub(crate) fn open(
    blob: &Blob<'_>,
    material: &[u8],
    context: &BTreeMap<String, String>,
) -> Option<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(material).ok()?;
    cipher
        .decrypt(
            &Nonce::from(blob.nonce),
            Payload {
                msg: blob.body,
                aad: &aad(context),
            },
        )
        .ok()
}

/// The MAC of `message` under `key`.
pub(crate) fn mac(algorithm: &str, key: &[u8], message: &[u8]) -> Option<Vec<u8>> {
    macro_rules! with {
        ($hash:ty) => {{
            let mut m = <Hmac<$hash> as hmac::KeyInit>::new_from_slice(key).ok()?;
            m.update(message);
            m.finalize().into_bytes().to_vec()
        }};
    }
    Some(match algorithm {
        "HMAC_SHA_224" => with!(Sha224),
        "HMAC_SHA_256" => with!(Sha256),
        "HMAC_SHA_384" => with!(Sha384),
        "HMAC_SHA_512" => with!(Sha512),
        _ => return None,
    })
}

/// Whether `tag` is the MAC of `message`, compared in constant time.
pub(crate) fn mac_valid(algorithm: &str, key: &[u8], message: &[u8], tag: &[u8]) -> bool {
    let Some(expected) = mac(algorithm, key, message) else {
        return false;
    };
    expected.len() == tag.len() && expected.iter().zip(tag).fold(0_u8, |d, (a, b)| d | (a ^ b)) == 0
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn context(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn a_blob_opens_with_its_key_and_context_and_nothing_else() {
        let key = [9_u8; 32];
        let ctx = context(&[("tenant", "a")]);
        let blob = seal("key-1", &key, &ctx, b"attack at dawn").unwrap();
        let parsed = parse(&blob).unwrap();
        assert_eq!(
            (parsed.key_id, parsed.material),
            ("key-1", material_id(&key))
        );
        assert_eq!(
            open(&parsed, &key, &ctx).as_deref(),
            Some(&b"attack at dawn"[..])
        );
        assert_eq!(open(&parsed, &key, &context(&[])), None);
        assert_eq!(open(&parsed, &key, &context(&[("tenant", "b")])), None);
        assert_eq!(open(&parsed, &[8_u8; 32], &ctx), None);
        assert!(parse(b"not ciphertext").is_none());
        assert!(parse(&blob[..10]).is_none());
    }

    #[test]
    fn hmac_sha_256_matches_the_rfc_4231_case_1() {
        let key = [0x0b_u8; 20];
        let tag = mac("HMAC_SHA_256", &key, b"Hi There").unwrap();
        assert_eq!(
            aws_core::hex::encode(&tag),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        assert!(mac_valid("HMAC_SHA_256", &key, b"Hi There", &tag));
        assert!(!mac_valid("HMAC_SHA_256", &key, b"Hi there", &tag));
        assert!(mac("HMAC_SHA_1", &key, b"x").is_none());
    }
}
