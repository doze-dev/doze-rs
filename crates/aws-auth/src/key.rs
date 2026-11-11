//! SigV4 key derivation and the HMAC/SHA-256 primitives (RustCrypto).
use aws_core::hex;
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// The derived signing key for one `(secret, date, region, service)`.
pub struct SigningKey([u8; 32]);

impl std::fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SigningKey(<redacted>)")
    }
}

fn hmac(key: &[u8], msg: &[u8]) -> Option<[u8; 32]> {
    let mut m = <HmacSha256 as KeyInit>::new_from_slice(key).ok()?;
    m.update(msg);
    Some(m.finalize().into_bytes().into())
}

impl SigningKey {
    /// `HMAC(HMAC(HMAC(HMAC("AWS4"+secret, date), region), service), "aws4_request")`.
    #[must_use]
    pub fn derive(secret: &str, date: &str, region: &str, service: &str) -> Option<Self> {
        let mut seed = Vec::with_capacity(4 + secret.len());
        seed.extend_from_slice(b"AWS4");
        seed.extend_from_slice(secret.as_bytes());
        let k = hmac(&seed, date.as_bytes())?;
        let k = hmac(&k, region.as_bytes())?;
        let k = hmac(&k, service.as_bytes())?;
        hmac(&k, b"aws4_request").map(Self)
    }

    /// Hex signature of `string_to_sign`.
    #[must_use]
    pub fn sign_hex(&self, string_to_sign: &[u8]) -> Option<String> {
        hmac(&self.0, string_to_sign).map(|s| hex::encode(&s))
    }

    /// Constant-time check of a claimed signature (raw bytes) over `string_to_sign`.
    #[must_use]
    pub fn verify(&self, string_to_sign: &[u8], claimed: &[u8]) -> bool {
        let Ok(mut m) = <HmacSha256 as KeyInit>::new_from_slice(&self.0) else {
            return false;
        };
        m.update(string_to_sign);
        m.verify_slice(claimed).is_ok()
    }

    /// [`Self::verify`] for a hex-encoded claim.
    #[must_use]
    pub fn verify_hex(&self, string_to_sign: &[u8], claimed_hex: &str) -> bool {
        hex::decode(claimed_hex).is_some_and(|c| self.verify(string_to_sign, &c))
    }
}

/// Hex SHA-256.
#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(&Sha256::digest(data))
}

/// SHA-256 of nothing, hex.
pub const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
