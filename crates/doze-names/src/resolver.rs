//! The resolver: a small DNS server answering the zone from the shared registry, and the take-over loop that makes
//! the zone survive any one program. With doze, doze-aws and doze-kafka as peers, the one holding the socket may exit
//! while others still run, so every peer keeps trying; whoever holds it answers for all of them.

use crate::Registry;
use crate::front::RESOLVER_KEY;
use crate::upstream::Upstreams;
use crate::wire::{RELAY_TTL, answer, cap_ttl};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// How often a peer that lost the bind tries again: prompt when the holder exits, free at idle.
const RETRY: Duration = Duration::from_secs(2);

/// What the server asks for each question.
pub type Resolve = Arc<dyn Fn(&str) -> Option<Ipv4Addr> + Send + Sync>;
/// Where the server says what it is doing ("standing by", "serving").
pub type Log = Arc<dyn Fn(String) + Send + Sync>;

/// How to serve.
#[derive(Clone)]
#[allow(missing_debug_implementations, reason = "it holds closures")]
pub struct ResolverConfig {
    pub addr: String,
    pub resolve: Resolve,
    /// The registry to record this process in while it holds the socket (so peers can say who serves, rather than
    /// blaming a stranger on the port).
    pub registry: Option<Registry>,
    pub upstreams: Arc<Upstreams>,
    pub log: Log,
}

impl ResolverConfig {
    /// Answers the zone from `reg`, at the zone's address ([`crate::resolver_addr`]), forwarding to the machine's
    /// upstreams.
    pub fn from_registry(reg: &Registry) -> ResolverConfig {
        let r = reg.clone();
        ResolverConfig {
            addr: crate::resolver_addr(),
            resolve: Arc::new(move |host| r.resolve(host)),
            registry: Some(reg.clone()),
            upstreams: Arc::new(Upstreams::system()),
            log: Arc::new(|_| {}),
        }
    }
}

/// The zone's DNS server for as long as this process holds the socket. Whether or not it ever wins the bind is
/// invisible to callers: the names resolve either way, because whoever holds it reads the same registry.
#[derive(Debug)]
pub struct Resolver {
    stop: watch::Sender<bool>,
    bound: watch::Receiver<bool>,
    task: JoinHandle<()>,
}

/// Starts serving (inside a tokio runtime). Never fails: standing by is the normal case for all peers but one.
pub fn serve(cfg: ResolverConfig) -> Resolver {
    let (stop, stop_rx) = watch::channel(false);
    let (bound_tx, bound) = watch::channel(false);
    let task = tokio::spawn(run(cfg, stop_rx, bound_tx));
    Resolver { stop, bound, task }
}

impl Resolver {
    /// Whether this process holds the socket within `within` (tests; production does not care which peer serves).
    pub async fn bound(&mut self, within: Duration) -> bool {
        tokio::time::timeout(within, self.bound.wait_for(|b| *b))
            .await
            .is_ok_and(|r| r.is_ok())
    }

    /// Stops serving and releases the socket, letting a peer take over.
    pub async fn close(self) {
        let _ = self.stop.send(true);
        let _ = self.task.await;
    }
}

async fn run(cfg: ResolverConfig, mut stop: watch::Receiver<bool>, bound: watch::Sender<bool>) {
    let mut announced = false;
    loop {
        match UdpSocket::bind(&cfg.addr).await {
            Ok(sock) => {
                // Recorded before Bound is released: whoever waited may ask the registry who has it at once.
                if let Some(r) = &cfg.registry {
                    let _ = r.claim_holder(RESOLVER_KEY, &cfg.addr);
                }
                let _ = bound.send(true);
                (cfg.log)(format!("names: serving {} on {}", crate::SUFFIX, cfg.addr));
                announced = false;
                let sock = Arc::new(sock);
                answer_until_stopped(Arc::clone(&sock), &cfg, &mut stop).await;
                // The record goes while the socket is still held: once it closes a peer may bind and record itself,
                // and a release after that would erase the successor's record (it matches by pid, and peers in one
                // process share one; seen on a slow Windows runner).
                if let Some(r) = &cfg.registry {
                    let _ = r.release_key(RESOLVER_KEY);
                }
                drop(sock);
                if *stop.borrow() {
                    return;
                }
            }
            Err(e) if !announced => {
                announced = true;
                (cfg.log)(why_not(&cfg, &e));
            }
            Err(_) => {}
        }
        tokio::select! {
            _ = stop.changed() => return,
            () = tokio::time::sleep(RETRY) => {}
        }
    }
}

/// Why this process is not serving, naming who is when the registry knows.
fn why_not(cfg: &ResolverConfig, e: &std::io::Error) -> String {
    let z = crate::SUFFIX;
    if e.kind() != std::io::ErrorKind::AddrInUse {
        return format!("names: cannot serve {z} ({e}); names will not resolve");
    }
    match cfg.registry.as_ref().and_then(Registry::resolver_holder) {
        Some(h) => format!(
            "names: {} is serving {z} (pid {}); standing by",
            h.owner, h.pid
        ),
        // Not a doze peer: on Linux usually systemd-resolved or dnsmasq on the wildcard.
        None => format!(
            "names: something other than doze holds {}, so {z} names will not resolve through it; apex names \
             still work through the hosts file",
            cfg.addr
        ),
    }
}

async fn answer_until_stopped(
    sock: Arc<UdpSocket>,
    cfg: &ResolverConfig,
    stop: &mut watch::Receiver<bool>,
) {
    let mut buf = vec![0u8; 512];
    loop {
        let (n, peer) = tokio::select! {
            _ = stop.changed() => return,
            r = sock.recv_from(&mut buf) => match r { Ok(v) => v, Err(_) => return },
        };
        let Some(q) = buf.get(..n).map(<[u8]>::to_vec) else {
            continue;
        };
        let Some((resp, forward)) = answer(&q, &*cfg.resolve) else {
            continue;
        };
        // Forwarding is for this machine, not for whoever can reach the socket: anyone else gets the NXDOMAIN.
        if !(forward && asked_from_here(peer.ip())) {
            let _ = sock.send_to(&resp, peer).await;
            continue;
        }
        let (sock, upstreams) = (sock.clone(), cfg.upstreams.clone());
        tokio::spawn(async move {
            let reply = upstreams.forward(&q).await.map(|r| cap_ttl(&r, RELAY_TTL));
            let _ = sock.send_to(reply.as_deref().unwrap_or(&resp), peer).await;
        });
    }
}

/// Whether a query came from this machine: loopback, the resolver's own address (Linux answers for its interface),
/// or a translated zone address (macOS).
fn asked_from_here(ip: IpAddr) -> bool {
    crate::on_this_machine(ip) || ip.to_string() == crate::RESOLVER_IP
}

/// A peer's address for tests: a free loopback UDP port nothing uses.
///
/// Picked below every system's ephemeral range (Linux starts at 32768, macOS and Windows at 49152). A port the OS
/// handed out and this function released is the next one it hands to someone else's `bind(":0")`: a test asking the
/// resolver from an ephemeral client socket could take the very port the resolver was about to bind, and the
/// take-over test failed that way under a parallel run.
pub fn free_udp_addr() -> std::io::Result<SocketAddr> {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seed = std::process::id().wrapping_mul(7919);
    for _ in 0..512 {
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let port = 20000 + u16::try_from(seed.wrapping_add(n.wrapping_mul(97)) % 12000).unwrap_or(0);
        if let Ok(s) = std::net::UdpSocket::bind(("127.0.0.1", port)) {
            return s.local_addr();
        }
    }
    std::net::UdpSocket::bind("127.0.0.1:0")?.local_addr()
}

#[cfg(test)]
#[path = "resolver_tests.rs"]
mod tests;
