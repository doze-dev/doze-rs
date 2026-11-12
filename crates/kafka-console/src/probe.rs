//! The advertised-listener check: the two steps a client takes, made from the broker's side with real dials.
//!
//! 1. Dial the listener and ask ApiVersions: does it answer?
//! 2. Ask Metadata (in process, as every console read is) what the broker says clients should reconnect to.
//! 3. Dial that address back and ask ApiVersions: can a client that was told it get there?
//!
//! Every dial has its own short deadline, and the whole check one of 1.5 s, so a page that shows it is never held longer.
//! It runs when asked ("Check again") and at a page load (a result under 30 s old is reused), never on a timer and never
//! for a fragment the stream refetches (those use [`Prober::peek`], which dials nothing). Callers that arrive while a
//! check is running share its result, so many at once are one set of dials. A check that did not finish claims nothing and
//! is not reused: the next page load tries again.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use kafka_wire::api::Response;
use kafka_wire::messages::ApiKey;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::backend::{Backend, requests};
use crate::facts::Facts;
use crate::model::{ProbeView, Rule, Verdict};

mod judge;

pub use judge::{Dial, Measured, Named, judge};

/// The longest a dial of the listener may take.
pub const LISTENER_DEADLINE: Duration = Duration::from_millis(700);
/// The longest a dial of the advertised address may take (a name may need resolving).
pub const ADVERTISED_DEADLINE: Duration = Duration::from_millis(1000);
/// The longest the whole check may take.
pub const DEADLINE: Duration = Duration::from_millis(1500);
/// How old a result may be and still be shown at page load.
pub const FRESH: Duration = Duration::from_secs(30);

/// The bytes of an ApiVersions v0 request: the oldest version, which every broker answers before anything else.
#[must_use]
pub fn api_versions_request(correlation_id: i32, client_id: &str) -> Vec<u8> {
    let id = client_id.as_bytes();
    let mut payload = Vec::with_capacity(10 + id.len());
    payload.extend_from_slice(&18_i16.to_be_bytes());
    payload.extend_from_slice(&0_i16.to_be_bytes());
    payload.extend_from_slice(&correlation_id.to_be_bytes());
    payload.extend_from_slice(&i16::try_from(id.len()).unwrap_or(0).to_be_bytes());
    payload.extend_from_slice(id);
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&i32::try_from(payload.len()).unwrap_or(0).to_be_bytes());
    frame.extend_from_slice(&payload);
    frame
}

/// Whether `body` (an ApiVersions response after its length prefix) answers `correlation_id` with no error.
#[must_use]
pub fn is_api_versions_answer(body: &[u8], correlation_id: i32) -> bool {
    let corr = body.first_chunk::<4>().map(|b| i32::from_be_bytes(*b));
    let error = body
        .get(4..)
        .and_then(|b| b.first_chunk::<2>())
        .map(|b| i16::from_be_bytes(*b));
    corr == Some(correlation_id) && error == Some(0)
}

/// What went wrong with a dial, in a few words (never the system's own text).
fn why(e: &std::io::Error) -> String {
    use std::io::ErrorKind::{ConnectionRefused, HostUnreachable, NetworkUnreachable, TimedOut};
    match e.kind() {
        ConnectionRefused => "connection refused".to_owned(),
        TimedOut => "timed out".to_owned(),
        HostUnreachable | NetworkUnreachable => "no route to host".to_owned(),
        _ => {
            let text = e.to_string().to_ascii_lowercase();
            let lookup = [
                "lookup",
                "name or service",
                "nodename",
                "no address associated",
                "resolve",
            ];
            if lookup.iter().any(|w| text.contains(w)) {
                "no such host".to_owned()
            } else {
                "could not connect".to_owned()
            }
        }
    }
}

async fn ask(stream: &mut TcpStream) -> Result<(), String> {
    const CORRELATION: i32 = 7;
    stream
        .write_all(&api_versions_request(CORRELATION, "console-check"))
        .await
        .map_err(|e| why(&e))?;
    let mut len = [0_u8; 4];
    stream
        .read_exact(&mut len)
        .await
        .map_err(|_| "connected, but it did not answer ApiVersions".to_owned())?;
    let len = usize::try_from(i32::from_be_bytes(len)).unwrap_or(usize::MAX);
    if !(6..=65_536).contains(&len) {
        return Err("connected, but what it sent is not Kafka".to_owned());
    }
    let mut body = vec![0_u8; len];
    stream
        .read_exact(&mut body)
        .await
        .map_err(|_| "connected, but it did not answer ApiVersions".to_owned())?;
    if is_api_versions_answer(&body, CORRELATION) {
        Ok(())
    } else {
        Err("connected, but it did not answer ApiVersions".to_owned())
    }
}

/// Dials `addr` (a name or an address and a port), and asks ApiVersions unless `tls` (a TLS listener is only dialled).
pub async fn dial(addr: &str, tls: bool, deadline: Duration) -> Dial {
    let start = Instant::now();
    let attempt = async {
        let mut stream = TcpStream::connect(addr).await.map_err(|e| why(&e))?;
        if tls { Ok(()) } else { ask(&mut stream).await }
    };
    let outcome = tokio::time::timeout(deadline, attempt).await;
    let ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    match outcome {
        Ok(Ok(())) => Dial {
            answered: true,
            ms,
            why: String::new(),
        },
        Ok(Err(why)) => Dial {
            answered: false,
            ms,
            why,
        },
        Err(_) => Dial {
            answered: false,
            ms,
            why: format!(
                "timed out after {}",
                crate::format::millis(u64::try_from(deadline.as_millis()).unwrap_or(0))
            ),
        },
    }
}

/// `host:port`, with brackets around an IPv6 literal.
fn join(host: &str, port: i32) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

/// What Metadata names as the broker.
async fn named(backend: &Backend) -> Named {
    match backend.call(requests::metadata_brokers()).await {
        Ok(Response::Metadata(m)) => match m.brokers.as_slice() {
            [one] => Named::Broker(join(one.host.as_str(), one.port)),
            [] => Named::Nothing("it named no broker".to_owned()),
            many => Named::Nothing(format!("it named {} brokers", many.len())),
        },
        _ => Named::Nothing("the call was not answered".to_owned()),
    }
}

/// The check, and the last result.
#[derive(Debug)]
pub struct Prober {
    facts: Facts,
    backend: Backend,
    last: Mutex<Option<(Instant, ProbeView)>>,
    /// Held while a check runs: one at a time.
    flight: tokio::sync::Mutex<()>,
    /// Checks finished so far: a caller that waited for the flight and sees this changed shares the result it left.
    finished: AtomicU64,
    /// How old a result may be and still be shown at page load.
    fresh: Duration,
}

impl Prober {
    #[must_use]
    pub fn new(facts: Facts, backend: Backend) -> Self {
        Self::with_fresh(facts, backend, FRESH)
    }

    /// A prober that reuses a result for `fresh` (a test's shorter [`FRESH`]).
    #[must_use]
    pub fn with_fresh(facts: Facts, backend: Backend, fresh: Duration) -> Self {
        Self {
            facts,
            backend,
            last: Mutex::new(None),
            flight: tokio::sync::Mutex::new(()),
            finished: AtomicU64::new(0),
            fresh,
        }
    }

    /// Runs the check now, unless one is running: then it waits for that one and shares its result. A check that does
    /// not finish in [`DEADLINE`] claims nothing.
    pub async fn run(&self) -> ProbeView {
        let before = self.finished.load(Ordering::Acquire);
        let _flight = self.flight.lock().await;
        if self.finished.load(Ordering::Acquire) != before
            && let Some((_, shared)) = self.last_result()
        {
            return shared;
        }
        let view = self.measure().await;
        *self
            .last
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((Instant::now(), view.clone()));
        self.finished.fetch_add(1, Ordering::Release);
        view
    }

    fn last_result(&self) -> Option<(Instant, ProbeView)> {
        self.last
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// What the last check said, however old, and never a dial; before any check, "Not checked".
    #[must_use]
    pub fn peek(&self) -> ProbeView {
        self.last_result()
            .map_or_else(|| self.unchecked(), |(_, v)| v)
    }

    fn unchecked(&self) -> ProbeView {
        let f = &self.facts;
        ProbeView {
            verdict: Verdict::NotChecked,
            rules: Vec::<Rule>::new(),
            listen: f.bootstrap(),
            advertised: f.advertised(),
            same: f.bootstrap() == f.advertised(),
            metadata_version: self.backend.version(ApiKey::Metadata),
        }
    }

    async fn measure(&self) -> ProbeView {
        let f = &self.facts;
        let measure = async {
            let (to_listener, to_advertised) = (f.dial_listener(), f.advertised());
            let (listener, metadata, advertised) = tokio::join!(
                dial(&to_listener, f.tls, LISTENER_DEADLINE),
                named(&self.backend),
                dial(&to_advertised, f.tls, ADVERTISED_DEADLINE),
            );
            Measured {
                listen: f.bootstrap(),
                metadata_version: self.backend.version(ApiKey::Metadata),
                advertised: f.advertised(),
                tls: f.tls,
                listener,
                metadata,
                advertised_dial: Some(advertised),
            }
        };
        match tokio::time::timeout(DEADLINE, measure).await {
            Ok(m) => judge(&m),
            Err(_) => self.unchecked(),
        }
    }

    /// The last result if it is fresh and says something, else a new one.
    pub async fn cached(&self) -> ProbeView {
        let kept = self
            .last_result()
            .filter(|(at, v)| at.elapsed() < self.fresh && v.verdict != Verdict::NotChecked)
            .map(|(_, v)| v);
        match kept {
            Some(v) => v,
            None => self.run().await,
        }
    }
}

#[cfg(test)]
mod tests;
