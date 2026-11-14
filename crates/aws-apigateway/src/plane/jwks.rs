//! An issuer's signing keys: fetched from `<issuer>/.well-known/openid-configuration` and its `jwks_uri` (over `https://`, or
//! `http://` for a local identity provider), kept per issuer, and a token's RSA signature verified against them.
//!
//! As AWS documents it: "API Gateway caches the public key for up to two hours on a best-effort basis. On a cache miss, API Gateway
//! fetches the public key from the issuer over the network"; the JWKS endpoint has 1,500 ms and its answer 150,000 bytes (HTTP API
//! quotas). A miss is a key id the kept set does not hold, or a set older than two hours. A set is fetched again for an unknown key
//! id at most once every 30 seconds (memory: AWS does not say), so a token with a made-up `kid` cannot make every request a fetch.
//! Nothing runs on a timer: the age is read when a token arrives.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::Value;

use super::client;
use crate::doc::text;

/// How long a fetched set is used.
const KEEP_MS: i64 = 2 * 60 * 60 * 1000;
/// The least time between fetches of one issuer's set for an unknown key id (memory).
const REFETCH_MS: i64 = 30 * 1000;
/// "Timeout for JSON Web Key Set endpoint: 1500 ms".
#[cfg(not(test))]
const FETCH_TIMEOUT: Duration = Duration::from_millis(1500);
/// The unit tests' debug-built TLS handshakes, under a whole workspace's test load, can take longer than AWS's 1,500 ms.
#[cfg(test)]
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
/// "Response size from JSON Web Key Set endpoint: 150000 bytes".
const MAX_ANSWER: usize = 150_000;

/// The kept sets, by issuer.
#[derive(Debug, Default)]
pub(crate) struct Jwks {
    by_issuer: Mutex<HashMap<String, Kept>>,
}

#[derive(Debug, Clone)]
struct Kept {
    keys: Arc<Vec<Value>>,
    fetched_ms: i64,
}

impl Jwks {
    /// The issuer's keys holding `kid` (any key when the token names none): the kept set when it is fresh and holds it, else a
    /// fetched one. `Err` says why there are none.
    pub(crate) async fn keys(
        &self,
        issuer: &str,
        kid: Option<&str>,
        now_ms: i64,
    ) -> Result<Arc<Vec<Value>>, String> {
        let kept = self
            .by_issuer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(issuer)
            .cloned();
        if let Some(k) = &kept {
            let fresh = now_ms - k.fetched_ms < KEEP_MS;
            if fresh && (holds(&k.keys, kid) || now_ms - k.fetched_ms < REFETCH_MS) {
                return Ok(Arc::clone(&k.keys));
            }
        }
        match fetch(issuer).await {
            Ok(keys) => {
                let keys = Arc::new(keys);
                self.by_issuer
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(
                        issuer.to_owned(),
                        Kept {
                            keys: Arc::clone(&keys),
                            fetched_ms: now_ms,
                        },
                    );
                Ok(keys)
            }
            // Best effort, as AWS's cache: a set that could not be refreshed is still used while it is fresh.
            Err(why) => match kept {
                Some(k) if now_ms - k.fetched_ms < KEEP_MS => Ok(k.keys),
                _ => Err(why),
            },
        }
    }
}

fn holds(keys: &[Value], kid: Option<&str>) -> bool {
    kid.is_none_or(|kid| keys.iter().any(|k| text(k, "kid") == Some(kid)))
}

async fn get_json(url: &str) -> Result<Value, String> {
    let out = client::Out {
        limit: MAX_ANSWER,
        ..client::Out::new("GET".into(), url.to_owned(), FETCH_TIMEOUT)
    };
    let answer = client::call(&out)
        .await
        .map_err(|e| format!("{url}: {e}"))?;
    if answer.status != 200 {
        return Err(format!("{url} answered {}", answer.status));
    }
    serde_json::from_slice(&answer.body).map_err(|_| format!("{url} did not answer JSON"))
}

/// The issuer's keys, from its discovery document's `jwks_uri`.
async fn fetch(issuer: &str) -> Result<Vec<Value>, String> {
    let config = get_json(&format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    ))
    .await?;
    let uri = text(&config, "jwks_uri").ok_or("the discovery document has no jwks_uri")?;
    let set = get_json(uri).await?;
    set.get("keys")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| format!("{uri} has no keys"))
}

/// Whether `signature` over `signed` verifies under the RSA JWK `{kty: RSA, n, e}` with the hash `alg` names (RS256, RS384, RS512).
pub(crate) fn rsa_verifies(jwk: &Value, alg: &str, signed: &[u8], signature: &[u8]) -> bool {
    if text(jwk, "kty") != Some("RSA") || text(jwk, "alg").is_some_and(|a| a != alg) {
        return false;
    }
    let (Some(n), Some(e)) = (
        text(jwk, "n").and_then(super::jwt::b64url),
        text(jwk, "e").and_then(super::jwt::b64url),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_key_id_is_found_or_missed() {
        let keys = vec![json!({"kid": "a"}), json!({"kid": "b"})];
        assert!(holds(&keys, Some("b")));
        assert!(!holds(&keys, Some("c")));
        assert!(holds(&keys, None));
        assert!(!rsa_verifies(&json!({"kty": "EC"}), "RS256", b"x", b"y"));
        assert!(!rsa_verifies(
            &json!({"kty": "RSA", "alg": "RS512", "n": "AQAB", "e": "AQAB"}),
            "RS256",
            b"x",
            b"y"
        ));
    }
}
