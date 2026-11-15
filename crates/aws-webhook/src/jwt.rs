//! JSON Web Tokens: taken apart, and their signature verified against one of the issuer's JSON Web Keys. Shared by API Gateway's
//! `JWT` authorizer and STS's `AssumeRoleWithWebIdentity`, which differ in the algorithms they accept: API Gateway "only RSA-based
//! algorithms" (RS256, RS384, RS512); STS RSA or ECDSA keys ("RS256, RS384, or RS512) or ECDSA keys (ES256, ES384, or ES512)", the
//! STS model's `WebIdentityToken`). The keys come from [`crate::jwks`].

use serde_json::Value;

use crate::jwks::Jwks;

/// The algorithms API Gateway's JWT authorizer accepts.
pub const RSA: &[&str] = &["RS256", "RS384", "RS512"];
/// The algorithms STS accepts for a web identity token.
pub const RSA_OR_ECDSA: &[&str] = &["RS256", "RS384", "RS512", "ES256", "ES384", "ES512"];

/// A token taken apart.
#[derive(Debug)]
pub struct Token {
    /// The header (`alg`, `kid`, ...).
    pub header: Value,
    /// The claims.
    pub claims: Value,
    signed: String,
    signature: Vec<u8>,
}

/// Why a token's signature is not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The token is signed with an algorithm the caller does not accept.
    Algorithm(String),
    /// The issuer's keys could not be fetched (why, for the log).
    Unfetchable(String),
    /// No key of the issuer has the token's key id.
    UnknownKey(String),
    /// No key of the issuer verifies the signature.
    BadSignature,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Algorithm(alg) => write!(f, "signing method {alg} is invalid"),
            Self::Unfetchable(_) => f.write_str("unable to fetch the issuer's public keys"),
            Self::UnknownKey(kid) => write!(f, "the key id {kid} is not in the issuer's JWKS"),
            Self::BadSignature => f.write_str("the token's signature is invalid"),
        }
    }
}

/// base64url without padding, as a token writes it.
#[must_use]
pub fn b64url(s: &str) -> Option<Vec<u8>> {
    let mut t = s.replace('-', "+").replace('_', "/");
    while !t.len().is_multiple_of(4) {
        t.push('=');
    }
    aws_core::base64::decode(&t)
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

// fuzz:parser fuzz_jwt
/// Reads a token (`header.claims.signature`, base64url; a `Bearer ` prefix is allowed).
#[must_use]
pub fn decode(token: &str) -> Option<Token> {
    let token = token.strip_prefix("Bearer ").unwrap_or(token).trim();
    let mut parts = token.split('.');
    let (h, c, s) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || token.len() > 16 * 1024 {
        return None;
    }
    Some(Token {
        header: serde_json::from_slice(&b64url(h)?).ok()?,
        claims: serde_json::from_slice(&b64url(c)?).ok()?,
        signed: format!("{h}.{c}"),
        signature: b64url(s)?,
    })
}

impl Token {
    /// The header's `alg`.
    #[must_use]
    pub fn alg(&self) -> &str {
        text(&self.header, "alg").unwrap_or_default()
    }

    /// The header's `kid`, when it names one.
    #[must_use]
    pub fn kid(&self) -> Option<&str> {
        text(&self.header, "kid")
    }

    /// Whether the signature verifies under one of `issuer`'s keys (fetched or kept by `jwks`), with an algorithm in `accepted`.
    ///
    /// # Errors
    /// Why it does not: see [`Refusal`].
    pub async fn verify(
        &self,
        jwks: &Jwks,
        issuer: &str,
        accepted: &[&str],
        now_ms: i64,
    ) -> Result<(), Refusal> {
        let alg = self.alg();
        if !accepted.contains(&alg) {
            return Err(Refusal::Algorithm(alg.to_owned()));
        }
        let kid = self.kid();
        let keys = jwks
            .keys(issuer, kid, now_ms)
            .await
            .map_err(Refusal::Unfetchable)?;
        let mut candidates = keys
            .iter()
            .filter(|k| kid.is_none() || text(k, "kid") == kid)
            .peekable();
        if candidates.peek().is_none() {
            return Err(Refusal::UnknownKey(kid.unwrap_or_default().to_owned()));
        }
        if candidates.any(|k| verifies(k, alg, self.signed.as_bytes(), &self.signature)) {
            Ok(())
        } else {
            Err(Refusal::BadSignature)
        }
    }
}

/// Whether `signature` over `signed` verifies under the JWK with algorithm `alg`.
#[must_use]
pub fn verifies(jwk: &Value, alg: &str, signed: &[u8], signature: &[u8]) -> bool {
    if text(jwk, "alg").is_some_and(|a| a != alg) {
        return false;
    }
    if alg.starts_with("RS") {
        rsa_verifies(jwk, alg, signed, signature)
    } else {
        ec_verifies(jwk, alg, signed, signature)
    }
}

/// Under an RSA JWK `{kty: RSA, n, e}`, PKCS #1 v1.5 with the hash `alg` names (RS256, RS384, RS512).
fn rsa_verifies(jwk: &Value, alg: &str, signed: &[u8], signature: &[u8]) -> bool {
    if text(jwk, "kty") != Some("RSA") {
        return false;
    }
    let (Some(n), Some(e)) = (
        text(jwk, "n").and_then(b64url),
        text(jwk, "e").and_then(b64url),
    ) else {
        return false;
    };
    let bits = |b: &[u8]| u32::try_from(b.len() * 8).unwrap_or(0);
    let (Ok(n), Ok(e)) = (
        rsa::BoxedUint::from_be_slice(&n, bits(&n)),
        rsa::BoxedUint::from_be_slice(&e, bits(&e).max(64)),
    ) else {
        return false;
    };
    let Ok(key) = rsa::PublicKey::new(n, e) else {
        return false;
    };
    match alg {
        "RS256" => key.pkcs1v15_verify::<rsa::sha2::Sha256>(signed, signature),
        "RS384" => key.pkcs1v15_verify::<rsa::sha2::Sha384>(signed, signature),
        "RS512" => key.pkcs1v15_verify::<rsa::sha2::Sha512>(signed, signature),
        _ => return false,
    }
    .is_ok()
}

macro_rules! ec {
    ($curve:ident) => {
        mod $curve {
            use ::$curve::ecdsa::signature::Verifier;
            use ::$curve::ecdsa::{Signature, VerifyingKey};

            /// Under the uncompressed point `x`, `y`, a JWS signature (`r || s`, fixed width) with the curve's hash.
            pub(super) fn verifies(x: &[u8], y: &[u8], signed: &[u8], signature: &[u8]) -> bool {
                let mut point = Vec::with_capacity(1 + x.len() + y.len());
                point.push(4);
                point.extend_from_slice(x);
                point.extend_from_slice(y);
                let (Ok(key), Ok(sig)) = (
                    VerifyingKey::from_sec1_bytes(&point),
                    Signature::from_slice(signature),
                ) else {
                    return false;
                };
                key.verify(signed, &sig).is_ok()
            }
        }
    };
}

ec!(p256);
ec!(p384);
ec!(p521);

/// Under an EC JWK `{kty: EC, crv, x, y}`: ES256 on P-256, ES384 on P-384, ES512 on P-521.
fn ec_verifies(jwk: &Value, alg: &str, signed: &[u8], signature: &[u8]) -> bool {
    if text(jwk, "kty") != Some("EC") {
        return false;
    }
    let (Some(x), Some(y)) = (
        text(jwk, "x").and_then(b64url),
        text(jwk, "y").and_then(b64url),
    ) else {
        return false;
    };
    match (alg, text(jwk, "crv")) {
        ("ES256", Some("P-256")) => p256::verifies(&x, &y, signed, signature),
        ("ES384", Some("P-384")) => p384::verifies(&x, &y, signed, signature),
        ("ES512", Some("P-521")) => p521::verifies(&x, &y, signed, signature),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use serde_json::json;

    fn enc(v: &[u8]) -> String {
        aws_core::base64::encode(v)
            .replace('+', "-")
            .replace('/', "_")
            .trim_end_matches('=')
            .to_owned()
    }

    #[test]
    fn tokens_are_taken_apart() {
        let t = format!(
            "Bearer {}.{}.c2ln",
            enc(json!({"alg": "RS256", "kid": "k"}).to_string().as_bytes()),
            enc(json!({"iss": "i", "exp": 100}).to_string().as_bytes())
        );
        let token = decode(&t).unwrap();
        assert_eq!(token.claims["iss"], "i");
        assert_eq!((token.alg(), token.kid()), ("RS256", Some("k")));
        assert!(decode("a.b").is_none());
        assert!(decode("!!.!!.!!").is_none());
    }

    #[test]
    fn a_key_of_the_wrong_kind_or_algorithm_verifies_nothing() {
        assert!(!verifies(&json!({"kty": "EC"}), "RS256", b"x", b"y"));
        assert!(!verifies(
            &json!({"kty": "RSA", "alg": "RS512", "n": "AQAB", "e": "AQAB"}),
            "RS256",
            b"x",
            b"y"
        ));
        assert!(!verifies(
            &json!({"kty": "EC", "crv": "P-384", "x": "AQAB", "y": "AQAB"}),
            "ES256",
            b"x",
            b"y"
        ));
    }

    #[test]
    fn an_es256_signature_verifies_under_its_jwk_and_not_after_a_change() {
        use ::p256::ecdsa::signature::Signer;
        use ::p256::ecdsa::{Signature, SigningKey};
        let key = SigningKey::from_slice(&[7u8; 32]).unwrap();
        let point = key.verifying_key().to_sec1_point(false);
        let raw = point.as_bytes();
        let (x, y) = (raw.get(1..33).unwrap(), raw.get(33..65).unwrap());
        let jwk = json!({"kty": "EC", "crv": "P-256", "x": enc(x), "y": enc(y)});
        let sig: Signature = key.sign(b"head.claims");
        let bytes = sig.to_bytes();
        assert!(verifies(&jwk, "ES256", b"head.claims", &bytes));
        assert!(!verifies(&jwk, "ES256", b"head.claimz", &bytes));
        assert!(!verifies(&jwk, "ES384", b"head.claims", &bytes));
    }
}
