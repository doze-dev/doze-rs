//! What every handler shares.

use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::facts::Facts;
use crate::limits::Limits;
use crate::ring::Recorder;
use crate::web::Bundle;

/// The state of one console.
#[derive(Debug)]
pub struct State {
    /// What the binary said of itself at start.
    pub facts: Facts,
    /// The Live record the gateway feeds.
    pub recorder: Arc<Recorder>,
    /// Where the console listens.
    pub addr: SocketAddr,
    /// `--console-allow-host`.
    pub allow_hosts: Vec<String>,
    /// The Lab's forced events kept so far (`lab_once`).
    pub once: crate::lab_once::History,
    /// Cancelled when the console is told to stop: every open stream ends with it.
    pub stop: CancellationToken,
    /// One permit per event stream that may still be opened (the cap is [`Limits::max_streams`]).
    pub streams: Arc<Semaphore>,
    /// This run, in the cursors the API hands out: a cursor of another run names nothing here.
    pub epoch: u64,
    /// The console's built files, made ready when the first page is asked for (a console nobody opens holds no table of them).
    bundle: OnceLock<Bundle>,
}

/// The state, shared.
pub type Shared = Arc<State>;

impl State {
    /// The state of a console listening on `addr`.
    #[must_use]
    pub fn new(
        facts: Facts,
        recorder: Arc<Recorder>,
        addr: SocketAddr,
        allow_hosts: Vec<String>,
        stop: CancellationToken,
        limits: &Limits,
    ) -> Shared {
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        Arc::new(Self {
            facts,
            recorder,
            addr,
            allow_hosts,
            once: crate::lab_once::History::default(),
            stop,
            streams: Arc::new(Semaphore::new(limits.max_streams)),
            epoch,
            bundle: OnceLock::new(),
        })
    }

    /// The console's built files.
    pub fn bundle(&self) -> &Bundle {
        self.bundle.get_or_init(Bundle::build)
    }

    /// Serves `bundle` in place of the embedded one, if no page was asked for yet (a test serves one with hashed assets, which
    /// the placeholder has none of); whether it took.
    pub fn serve_bundle(&self, bundle: Bundle) -> bool {
        self.bundle.set(bundle).is_ok()
    }

    /// The console as a caller on the services' bus, when it was given one.
    #[must_use]
    pub fn bus(&self) -> Option<crate::resources::bus::Bus> {
        let peers = self.facts.peers.clone()?;
        Some(
            crate::resources::bus::Bus::new(
                peers,
                &self.facts.account,
                &crate::regions::current(&self.facts.region),
                &self.facts.endpoint,
            )
            .recording(Arc::clone(&self.recorder)),
        )
    }

    /// The URL a browser on this machine reaches the console at (a wildcard address is reached on loopback).
    #[must_use]
    pub fn url(&self) -> String {
        url_of(self.addr)
    }
}

/// The URL a browser on this machine reaches a console listening on `addr` at.
#[must_use]
pub fn url_of(addr: SocketAddr) -> String {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    let ip = match addr.ip() {
        IpAddr::V4(v4) if v4.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(v6) if v6.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    format!("http://{}", SocketAddr::new(ip, addr.port()))
}
