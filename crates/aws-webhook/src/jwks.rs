//! An issuer's signing keys: fetched from `<issuer>/.well-known/openid-configuration` and its `jwks_uri` with this crate's client
//! (`https://` against the machine's trust store and `--ca-file`, or `http://` when the caller allows it), kept per issuer.
//!
//! As AWS documents it for API Gateway: "API Gateway caches the public key for up to two hours on a best-effort basis. On a cache
//! miss, API Gateway fetches the public key from the issuer over the network". STS documents no figure; the same two hours are used
//! (memory). A miss is a key id the kept set does not hold, or a set older than two hours. A set is fetched again for an unknown key
//! id at most once every 30 seconds (memory: AWS does not say), so a token with a made-up `kid` cannot make every request a fetch.
//! Nothing runs on a timer: the age is read when a token arrives.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::Value;

use crate::Request;

/// How long a fetched set is used.
const KEEP_MS: i64 = 2 * 60 * 60 * 1000;
/// The least time between fetches of one issuer's set for an unknown key id (memory).
const REFETCH_MS: i64 = 30 * 1000;

/// The kept sets, by issuer.
#[derive(Debug)]
pub struct Jwks {
    by_issuer: Mutex<HashMap<String, Kept>>,
    /// How long one fetch may take.
    timeout: Duration,
    /// The most bytes of one answer read.
    limit: usize,
}

#[derive(Debug, Clone)]
struct Kept {
    keys: Arc<Vec<Value>>,
    fetched_ms: i64,
}

impl Jwks {
    /// An empty cache whose fetches take at most `timeout` and read at most `limit` bytes of each answer.
    #[must_use]
    pub fn new(timeout: Duration, limit: usize) -> Self {
        Self {
            by_issuer: Mutex::new(HashMap::new()),
            timeout,
            limit,
        }
    }

    /// The issuer's keys holding `kid` (any key when the token names none): the kept set when it is fresh and holds it, else a
    /// fetched one.
    ///
    /// # Errors
    /// Why there are none (the fetch failed, and no fresh set is kept).
    pub async fn keys(
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
        match self.fetch(issuer).await {
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

    async fn get_json(&self, url: &str) -> Result<Value, String> {
        let r = Request {
            timeout: self.timeout,
            limit: self.limit,
            ..Request::new("GET", url)
        };
        let answer = crate::fetch(&r).await.map_err(|e| format!("{url}: {e}"))?;
        if answer.status != 200 {
            return Err(format!("{url} answered {}", answer.status));
        }
        serde_json::from_slice(&answer.body).map_err(|_| format!("{url} did not answer JSON"))
    }

    /// The issuer's keys, from its discovery document's `jwks_uri`.
    async fn fetch(&self, issuer: &str) -> Result<Vec<Value>, String> {
        let config = self
            .get_json(&format!(
                "{}/.well-known/openid-configuration",
                issuer.trim_end_matches('/')
            ))
            .await?;
        let uri = config
            .get("jwks_uri")
            .and_then(Value::as_str)
            .ok_or("the discovery document has no jwks_uri")?;
        let set = self.get_json(uri).await?;
        set.get("keys")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| format!("{uri} has no keys"))
    }
}

fn holds(keys: &[Value], kid: Option<&str>) -> bool {
    kid.is_none_or(|kid| {
        keys.iter()
            .any(|k| k.get("kid").and_then(Value::as_str) == Some(kid))
    })
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
    }
}
