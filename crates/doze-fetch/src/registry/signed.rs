//! The registry's signing scheme, one rule for every file it signs: an ed25519 signature, base64, over the lowercase
//! hex SHA-256 of the signed bytes (the canonical payload of an index or of the release file; an artifact's own sha256
//! for an artifact). The key is built into doze; a registry host is trusted for nothing.

use base64ct::{Base64, Encoding};
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use std::fmt;

/// The registry's public key (`registry/doze/keys.json`), the same one the Go doze carries. Rotating it takes a
/// release of doze.
pub const KEY: &str = "Nu5y8mbjYv7XEtNFBXwPVl+C8SBteVApIROefBwGWjw=";

/// Why a signature was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    Unsigned,
    Malformed,
    WrongKey,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Refused::Unsigned => "it is not signed",
            Refused::Malformed => "its signature is not a signature",
            Refused::WrongKey => {
                "its signature does not match the registry key built into this doze"
            }
        })
    }
}

/// A base64 ed25519 public key (the `keys.json` form).
pub fn parse_key(b64: &str) -> Option<VerifyingKey> {
    let raw = Base64::decode_vec(b64.trim()).ok()?;
    let bytes: [u8; 32] = raw.try_into().ok()?;
    VerifyingKey::from_bytes(&bytes).ok()
}

/// The key built into this doze.
pub fn builtin_key() -> Option<VerifyingKey> {
    parse_key(KEY)
}

/// Lowercase hex SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Checks `sig_b64` over the hex SHA-256 of `payload`.
pub fn verify(key: &VerifyingKey, payload: &str, sig_b64: &str) -> Result<(), Refused> {
    verify_hex(key, &sha256_hex(payload.as_bytes()), sig_b64)
}

/// Checks `sig_b64` over an already hashed `sha_hex` (an artifact's sha256 as the index lists it).
pub fn verify_hex(key: &VerifyingKey, sha_hex: &str, sig_b64: &str) -> Result<(), Refused> {
    if sig_b64.trim().is_empty() {
        return Err(Refused::Unsigned);
    }
    let raw = Base64::decode_vec(sig_b64.trim()).map_err(|_| Refused::Malformed)?;
    let sig = Signature::from_slice(&raw).map_err(|_| Refused::Malformed)?;
    key.verify_strict(sha_hex.to_ascii_lowercase().as_bytes(), &sig)
        .map_err(|_| Refused::WrongKey)
}

/// A short form of a key for people: the first and last bytes, as the proposal's `modules verify` shows it.
pub fn fingerprint(key: &VerifyingKey) -> String {
    let h = hex::encode(key.as_bytes());
    format!(
        "{}…{}",
        h.get(..4).unwrap_or_default(),
        h.get(h.len().saturating_sub(2)..).unwrap_or_default()
    )
}
