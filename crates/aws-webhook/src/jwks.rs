//! An issuer's signing keys: fetched from `<issuer>/.well-known/openid-configuration` and its `jwks_uri` with this crate's client
//! (`https://` against the machine's trust store and `--ca-file`, or `http://` when the caller allows it), kept per issuer.
//!
//! As AWS documents it for API Gateway: "API Gateway caches the public key for up to two hours on a best-effort basis. On a cache
//! miss, API Gateway fetches the public key from the issuer over the network". STS documents no figure; the same two hours are used
//! (memory). A miss is a key id the kept set does not hold, or a set older than two hours. A set is fetched again for an unknown key
//! id at most once every 30 seconds (memory: AWS does not say), so a token with a made-up `kid` cannot make every request a fetch.
//! Nothing runs on a timer: the age is read when a token arrives.
//!
//! **One fetch at a time per issuer**: requests that miss together wait for the one fetch and use its keys. **A failed fetch is
//! remembered for [`FAILED_MS`]** (ours: a moment, so an issuer that comes back is soon used), so an issuer that is down, with nothing
//! kept, costs one attempt in that time and not one per request.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::Value;
use tokio::sync::Mutex as Gate;

use crate::Request;

/// How long a fetched set is used.
const KEEP_MS: i64 = 2 * 60 * 60 * 1000;
/// The least time between fetches of one issuer's set for an unknown key id (memory).
const REFETCH_MS: i64 = 30 * 1000;
/// How long a failed fetch is answered from memory.
pub const FAILED_MS: i64 = 5 * 1000;

/// The kept sets, by issuer.
#[derive(Debug)]
pub struct Jwks {
    by_issuer: Mutex<HashMap<String, Issuer>>,
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

/// What is known of one issuer.
#[derive(Debug, Default)]
struct Issuer {
    kept: Option<Kept>,
    /// When the last fetch failed, and why.
    failed: Option<(i64, String)>,
    /// Held while its keys are fetched.
    gate: Arc<Gate<()>>,
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
        if let Some(known) = self.known(issuer, kid, now_ms) {
            return known;
        }
        let gate = Arc::clone(
            &self
                .by_issuer
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .entry(issuer.to_owned())
                .or_default()
                .gate,
        );
        let _turn = gate.lock().await;
        // Whoever held the gate may have fetched what this request needs.
        if let Some(known) = self.known(issuer, kid, now_ms) {
            return known;
        }
        let fetched = self.fetch(issuer).await;
        let mut all = self
            .by_issuer
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let entry = all.entry(issuer.to_owned()).or_default();
        match fetched {
            Ok(keys) => {
                let keys = Arc::new(keys);
                entry.kept = Some(Kept {
                    keys: Arc::clone(&keys),
                    fetched_ms: now_ms,
                });
                entry.failed = None;
                Ok(keys)
            }
            Err(why) => {
                entry.failed = Some((now_ms, why.clone()));
                fresh_kept(entry, now_ms).ok_or(why)
            }
        }
    }

    /// The answer memory gives without a fetch: a fresh set that holds the key (or was fetched a moment ago), or a fetch that failed a
    /// moment ago.
    fn known(
        &self,
        issuer: &str,
        kid: Option<&str>,
        now_ms: i64,
    ) -> Option<Result<Arc<Vec<Value>>, String>> {
        let all = self
            .by_issuer
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let entry = all.get(issuer)?;
        if let Some(k) = &entry.kept {
            let fresh = now_ms - k.fetched_ms < KEEP_MS;
            if fresh && (holds(&k.keys, kid) || now_ms - k.fetched_ms < REFETCH_MS) {
                return Some(Ok(Arc::clone(&k.keys)));
            }
        }
        let (at, why) = entry.failed.as_ref()?;
        (now_ms - at < FAILED_MS).then(|| fresh_kept(entry, now_ms).ok_or_else(|| why.clone()))
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

/// Best effort, as AWS's cache: a set that could not be refreshed is still used while it is fresh.
fn fresh_kept(entry: &Issuer, now_ms: i64) -> Option<Arc<Vec<Value>>> {
    entry
        .kept
        .as_ref()
        .filter(|k| now_ms - k.fetched_ms < KEEP_MS)
        .map(|k| Arc::clone(&k.keys))
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

    /// A plain issuer that answers after a moment, `500` when `down`; it counts the requests it is sent.
    async fn issuer(down: bool) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (count, me) = (Arc::clone(&seen), base.clone());
        tokio::spawn(async move {
            while let Ok((mut s, _)) = listener.accept().await {
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let me = me.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let asked = String::from_utf8_lossy(&buf[..n]).into_owned();
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    let body = if asked.contains("openid-configuration") {
                        json!({"jwks_uri": format!("{me}/keys")}).to_string()
                    } else {
                        json!({"keys": [{"kid": "a"}]}).to_string()
                    };
                    let status = if down { "500 Oops" } else { "200 OK" };
                    let answer = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    );
                    drop(s.write_all(answer.as_bytes()).await);
                });
            }
        });
        (base, seen)
    }

    #[tokio::test]
    async fn requests_that_miss_together_share_one_fetch() {
        let (base, seen) = issuer(false).await;
        let jwks = Arc::new(Jwks::new(Duration::from_secs(5), 1 << 20));
        let mut asks = Vec::new();
        for _ in 0..10 {
            let (jwks, base) = (Arc::clone(&jwks), base.clone());
            asks.push(tokio::spawn(async move {
                jwks.keys(&base, Some("a"), 1_000).await
            }));
        }
        for a in asks {
            assert_eq!(a.await.unwrap().unwrap().len(), 1);
        }
        // The discovery document and the key set, once: before, each request fetched both.
        assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn an_issuer_that_is_down_is_asked_once_in_a_while_not_on_every_request() {
        let (base, seen) = issuer(true).await;
        let jwks = Jwks::new(Duration::from_secs(5), 1 << 20);
        for now in [1_000, 1_001, 1_000 + FAILED_MS - 1] {
            let why = jwks.keys(&base, Some("a"), now).await.unwrap_err();
            assert!(why.contains("answered 500"), "{why}");
        }
        assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(
            jwks.keys(&base, Some("a"), 1_000 + FAILED_MS)
                .await
                .is_err()
        );
        assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
}
