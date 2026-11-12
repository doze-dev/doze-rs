//! What a `SecureString` is stored as: AES-256-GCM under a key made once for this data directory.
//!
//! AWS encrypts with a KMS key (`alias/aws/ssm` unless the caller names one). Here the key is the service's own: the value is
//! unreadable on disk and in a plain `GetParameter`, and readable with `WithDecryption`, which is the behaviour a caller can see.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};

const NONCE: usize = 12;
/// The key's size in bytes.
pub(crate) const KEY: usize = 32;

#[derive(Debug)]
pub(crate) struct Sealer {
    cipher: Aes256Gcm,
}

impl Sealer {
    /// A sealer for `key`, which must be [`KEY`] bytes.
    pub(crate) fn new(key: &[u8]) -> Option<Self> {
        Aes256Gcm::new_from_slice(key)
            .ok()
            .map(|cipher| Self { cipher })
    }

    pub(crate) fn seal(&self, plain: &[u8]) -> Option<Vec<u8>> {
        let mut nonce = [0_u8; NONCE];
        getrandom::fill(&mut nonce).ok()?;
        let sealed = self.cipher.encrypt(&Nonce::from(nonce), plain).ok()?;
        let mut out = nonce.to_vec();
        out.extend_from_slice(&sealed);
        Some(out)
    }

    pub(crate) fn open(&self, sealed: &[u8]) -> Option<Vec<u8>> {
        let (nonce, body) = sealed.split_at_checked(NONCE)?;
        let nonce: [u8; NONCE] = nonce.try_into().ok()?;
        self.cipher.decrypt(&Nonce::from(nonce), body).ok()
    }
}

/// A new random key.
pub(crate) fn new_key() -> Option<Vec<u8>> {
    let mut key = vec![0_u8; KEY];
    getrandom::fill(&mut key).ok()?;
    Some(key)
}
