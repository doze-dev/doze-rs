//! The one place this crate touches a cryptography library.
//!
//! Hashes, HMAC and the constant-time comparison come from the RustCrypto crates (`sha2`, `hmac`, `subtle`);
//! nothing else in the crate names them, so a different backend is a change to this file alone. PBKDF2 and
//! base64 are small and written here, on top of these functions, and pinned by published test vectors.

pub mod base64;
pub mod pbkdf2;

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256, Sha512};
use subtle::ConstantTimeEq;

/// HMAC-SHA-256 of the concatenation of `parts`, under `key` (a key of any length).
#[must_use]
pub fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    // HMAC accepts a key of any length, so this cannot fail; if it ever did, the all-zero answer matches no
    // proof and a login fails closed.
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(key) else {
        return [0; 32];
    };
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

/// HMAC-SHA-512 of the concatenation of `parts`, under `key` (a key of any length).
#[must_use]
pub fn hmac_sha512(key: &[u8], parts: &[&[u8]]) -> [u8; 64] {
    let Ok(mut mac) = Hmac::<Sha512>::new_from_slice(key) else {
        return [0; 64];
    };
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

/// SHA-256 of `data`.
#[must_use]
pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// SHA-512 of `data`.
#[must_use]
pub fn sha512(data: &[u8]) -> [u8; 64] {
    Sha512::digest(data).into()
}

/// Whether `a` and `b` are the same bytes, in time that depends on their length and not on where they differ.
/// Slices of different lengths are not equal (and say so at once, as every constant-time comparison does).
#[must_use]
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

#[cfg(test)]
pub(crate) mod tests;
