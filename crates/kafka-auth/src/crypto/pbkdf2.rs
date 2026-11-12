//! PBKDF2 (RFC 8018) with HMAC-SHA-256 and HMAC-SHA-512, for one output block.
//!
//! SCRAM asks for exactly as many bytes as the hash makes (RFC 5802 `Hi`), so a single block is all that is
//! needed: `U1 = PRF(password, salt || INT(1))`, `Ui = PRF(password, Ui-1)`, and the result is the XOR of them all.

use std::num::NonZeroU32;

use super::{hmac_sha256, hmac_sha512};

/// PBKDF2-HMAC-SHA-256, 32 bytes out.
#[must_use]
pub fn sha256(password: &[u8], salt: &[u8], iterations: NonZeroU32) -> [u8; 32] {
    derive(hmac_sha256, password, salt, iterations)
}

/// PBKDF2-HMAC-SHA-512, 64 bytes out.
#[must_use]
pub fn sha512(password: &[u8], salt: &[u8], iterations: NonZeroU32) -> [u8; 64] {
    derive(hmac_sha512, password, salt, iterations)
}

fn derive<const N: usize>(
    prf: fn(&[u8], &[&[u8]]) -> [u8; N],
    password: &[u8],
    salt: &[u8],
    iterations: NonZeroU32,
) -> [u8; N] {
    let mut last = prf(password, &[salt, &1u32.to_be_bytes()]);
    let mut out = last;
    for _ in 1..iterations.get() {
        last = prf(password, &[&last]);
        for (o, l) in out.iter_mut().zip(last.iter()) {
            *o ^= l;
        }
    }
    out
}

#[cfg(test)]
mod tests;
