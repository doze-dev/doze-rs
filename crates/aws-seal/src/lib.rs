//! How a service keeps a value it was told to keep secret (an SSM `SecureString`, a Secrets Manager secret): AES-256-GCM under a
//! key made once for the data directory, which the service stores beside the values.
//!
//! AWS encrypts with a KMS key. Here the key is the service's own: the value is unreadable on disk and in a plain read, and
//! readable when the caller asks for it open, which is the behaviour a caller can see.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};

const NONCE: usize = 12;
/// The key's size in bytes.
pub const KEY: usize = 32;

/// A key, ready to seal and open.
#[derive(Debug)]
pub struct Sealer {
    cipher: Aes256Gcm,
}

impl Sealer {
    /// A sealer for `key`, which must be [`KEY`] bytes.
    #[must_use]
    pub fn new(key: &[u8]) -> Option<Self> {
        Aes256Gcm::new_from_slice(key)
            .ok()
            .map(|cipher| Self { cipher })
    }

    #[must_use]
    pub fn seal(&self, plain: &[u8]) -> Option<Vec<u8>> {
        let mut nonce = [0_u8; NONCE];
        getrandom::fill(&mut nonce).ok()?;
        let sealed = self.cipher.encrypt(&Nonce::from(nonce), plain).ok()?;
        let mut out = nonce.to_vec();
        out.extend_from_slice(&sealed);
        Some(out)
    }

    #[must_use]
    pub fn open(&self, sealed: &[u8]) -> Option<Vec<u8>> {
        let (nonce, body) = sealed.split_at_checked(NONCE)?;
        let nonce: [u8; NONCE] = nonce.try_into().ok()?;
        self.cipher.decrypt(&Nonce::from(nonce), body).ok()
    }
}

/// A new random key.
#[must_use]
pub fn new_key() -> Option<Vec<u8>> {
    let mut key = vec![0_u8; KEY];
    getrandom::fill(&mut key).ok()?;
    Some(key)
}

/// `n` random bytes.
#[must_use]
pub fn random_bytes<const N: usize>() -> Option<[u8; N]> {
    let mut out = [0_u8; N];
    getrandom::fill(&mut out).ok()?;
    Some(out)
}

/// A random number below `n` (`n` above zero), with no bias.
#[must_use]
pub fn random_below(n: usize) -> Option<usize> {
    let n = u64::try_from(n).ok().filter(|n| *n > 0)?;
    // Reject the top of the range that would make some results likelier than others.
    let zone = u64::MAX - (u64::MAX % n);
    loop {
        let x = u64::from_le_bytes(random_bytes::<8>()?);
        if x < zone {
            return usize::try_from(x % n).ok();
        }
    }
}

/// A version 4 UUID, as text.
#[must_use]
pub fn uuid_v4() -> Option<String> {
    let mut b = random_bytes::<16>()?;
    if let Some(x) = b.get_mut(6) {
        *x = (*x & 0x0f) | 0x40;
    }
    if let Some(x) = b.get_mut(8) {
        *x = (*x & 0x3f) | 0x80;
    }
    let hex: Vec<String> = b.iter().map(|x| format!("{x:02x}")).collect();
    let h = |r: std::ops::Range<usize>| hex.get(r).map(|s| s.concat());
    Some(format!(
        "{}-{}-{}-{}-{}",
        h(0..4)?,
        h(4..6)?,
        h(6..8)?,
        h(8..10)?,
        h(10..16)?
    ))
}

/// `n` random letters and digits.
#[must_use]
pub fn alphanumeric(n: usize) -> Option<String> {
    const SET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    (0..n)
        .map(|_| SET.get(random_below(SET.len())?).map(|b| char::from(*b)))
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn a_sealed_value_opens_only_with_its_key() {
        let a = Sealer::new(&new_key().unwrap()).unwrap();
        let b = Sealer::new(&new_key().unwrap()).unwrap();
        let sealed = a.seal(b"hunter2").unwrap();
        assert_ne!(sealed, b"hunter2");
        assert_eq!(a.open(&sealed).as_deref(), Some(&b"hunter2"[..]));
        assert_eq!(b.open(&sealed), None);
        assert_eq!(a.open(b"short"), None);
    }

    #[test]
    fn a_uuid_has_its_shape() {
        let id = uuid_v4().unwrap();
        assert_eq!(id.len(), 36);
        assert_eq!(id.matches('-').count(), 4);
        assert_eq!(id.get(14..15), Some("4"));
        assert!(matches!(id.get(19..20), Some("8" | "9" | "a" | "b")));
    }

    #[test]
    fn random_below_stays_below() {
        assert!((0..1000).all(|_| random_below(7).is_some_and(|x| x < 7)));
        assert_eq!(random_below(0), None);
        assert_eq!(alphanumeric(6).map(|s| s.len()), Some(6));
    }
}
