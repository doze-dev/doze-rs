//! The shared front door: one wildcard listener on :80 and one on :443 for every doze program, routing by Host and
//! SNI. A name should mean one URL (`http://aws.doze`, not `:4566` in one mode and port-less in another), and macOS
//! binds a privileged port only on the wildcard, so the port is shared the way the resolver is: first-come, take-over
//! when the holder exits, and the holder serving every peer's routes from the one registry, so a process that dies
//! stops being proxied to the moment it stops answering DNS.

use crate::certs::{CA_CHECK_EVERY, CaSource, Certs, Chosen, Refused};
use crate::front::{INGRESS_KEY, INGRESS_TLS_KEY, ingress_bind, ingress_tls_bind};
use crate::resolver::Log;
use crate::standby::Standby;
use crate::{Registry, SUFFIX};
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_rustls::LazyConfigAcceptor;

/// The https side: the settings every handshake shares, and where its certificate comes from. The certificate is
/// chosen after the client's hello is read, on a blocking thread (it may read files or mint a leaf), and handed to
/// that one handshake, so a slow mint holds up neither the async workers nor other names.
#[derive(Debug, Clone)]
struct Tls {
    base: Arc<rustls::ServerConfig>,
    certs: Arc<Certs>,
    /// The names a refusal was logged for: each said once (bounded), not at every retry of a client.
    said: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
}

/// How many refused names are remembered as said before the memory starts again.
const SAID_CAP: usize = 256;

impl Tls {
    /// Whether a refusal for `name` is new, and so worth a line in the log.
    fn first_refusal(&self, name: &str) -> bool {
        let Ok(mut said) = self.said.lock() else {
            return false;
        };
        if said.len() >= SAID_CAP {
            said.clear();
        }
        said.insert(name.to_owned())
    }
}

/// The log line for a refused https handshake: which name, and why.
pub(crate) fn refusal_line(name: &str, why: &Refused) -> String {
    format!("names: https://{name} refused: {why}")
}

/// Where to serve, and from what.
#[derive(Clone)]
#[allow(missing_debug_implementations, reason = "it holds a closure")]
pub struct FrontConfig {
    pub registry: Registry,
    pub plain: String,
    pub tls: String,
    /// The certificate authority's folder (it may not exist yet: it is followed as it appears).
    pub ca_dir: std::path::PathBuf,
    pub ca_check_every: Duration,
    pub log: Log,
}

impl FrontConfig {
    /// The zone's front door: :80 and :443 (or as moved by the environment), the authority under the doze home.
    pub fn for_zone(reg: &Registry, ca_dir: &Path) -> FrontConfig {
        FrontConfig {
            registry: reg.clone(),
            plain: ingress_bind(),
            tls: ingress_tls_bind(),
            ca_dir: ca_dir.to_path_buf(),
            ca_check_every: CA_CHECK_EVERY,
            log: Arc::new(|_| {}),
        }
    }
}

/// The front door while this process holds its ports. Like the resolver it never fails: standing by is normal.
#[derive(Debug)]
pub struct FrontDoor {
    stop: watch::Sender<bool>,
    bound: watch::Receiver<bool>,
    tasks: Vec<JoinHandle<()>>,
}

/// Starts fronting (inside a tokio runtime).
pub fn serve(cfg: FrontConfig) -> FrontDoor {
    let (stop, stop_rx) = watch::channel(false);
    let (bound_tx, bound) = watch::channel(false);
    let certs = Arc::new(Certs::new(
        cfg.registry.clone(),
        CaSource::new(&cfg.ca_dir, cfg.ca_check_every),
    ));
    let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map(|b| {
        let mut c = b
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(Chosen(None)));
        c.alpn_protocols = vec![b"http/1.1".to_vec()];
        Tls {
            base: Arc::new(c),
            certs,
            said: Arc::default(),
        }
    })
    .ok();
    let plain = tokio::spawn(hold(
        cfg.clone(),
        cfg.plain.clone(),
        INGRESS_KEY,
        None,
        stop_rx.clone(),
        Some(bound_tx),
    ));
    let mut tasks = vec![plain];
    if let Some(acceptor) = tls {
        tasks.push(tokio::spawn(hold(
            cfg.clone(),
            cfg.tls.clone(),
            INGRESS_TLS_KEY,
            Some(acceptor),
            stop_rx,
            None,
        )));
    }
    FrontDoor { stop, bound, tasks }
}

impl FrontDoor {
    /// Whether this process holds the plain port within `within`.
    pub async fn bound(&mut self, within: Duration) -> bool {
        tokio::time::timeout(within, self.bound.wait_for(|b| *b))
            .await
            .is_ok_and(|r| r.is_ok())
    }

    /// Stops fronting and releases the ports for a peer to take.
    pub async fn close(self) {
        let _ = self.stop.send(true);
        for t in self.tasks {
            let _ = t.await;
        }
    }
}

/// Takes `bind` when free and serves on it until stopped, standing by and trying again while someone has it.
async fn hold(
    cfg: FrontConfig,
    bind: String,
    key: &'static str,
    tls: Option<Tls>,
    mut stop: watch::Receiver<bool>,
    bound: Option<watch::Sender<bool>>,
) {
    let mut announced = false;
    let mut standby = Standby::new(Some(cfg.registry.path()));
    loop {
        // IPv4 explicitly: `:80` takes the IPv6 wildcard, which macOS hands over even while another program holds
        // IPv4 0.0.0.0:80, leaving a socket no client reaches.
        match TcpListener::bind(&bind).await {
            Ok(listener) => {
                let _ = cfg.registry.claim_holder(key, &bind);
                if let Some(b) = &bound {
                    let _ = b.send(true);
                }
                (cfg.log)(format!("names: fronting {SUFFIX} on {bind}"));
                announced = false;
                accept_until_stopped(&cfg, &listener, tls.as_ref(), &mut stop).await;
                // Only this listener's own record, while it still holds the port (see the resolver's `run`).
                let _ = cfg.registry.release_holder(key, &bind);
                if *stop.borrow() {
                    return;
                }
            }
            Err(e) if !announced => {
                announced = true;
                (cfg.log)(why_not(&cfg.registry, &bind, key, &e));
            }
            Err(_) => {}
        }
        if standby.wait(&mut stop).await {
            return;
        }
    }
}

fn why_not(reg: &Registry, bind: &str, key: &str, e: &std::io::Error) -> String {
    if e.kind() != std::io::ErrorKind::AddrInUse {
        // On Linux usually the privileged-port sysctl, which setup applies.
        return format!("names: cannot bind {bind} ({e}); names will need their port");
    }
    match reg.snapshot().remove(key) {
        Some(h) => format!(
            "names: {} is fronting {bind} (pid {}); standing by",
            h.owner, h.pid
        ),
        // Something else answers doze names with its own site there, which reads as doze serving the wrong thing.
        None => format!(
            "names: something other than doze holds {bind}, so {SUFFIX} names cannot be port-less there; they \
             still work with their port. Stop it, or use the port-ful URL"
        ),
    }
}

async fn accept_until_stopped(
    cfg: &FrontConfig,
    listener: &TcpListener,
    tls: Option<&Tls>,
    stop: &mut watch::Receiver<bool>,
) {
    loop {
        let (stream, peer) = tokio::select! {
            _ = stop.changed() => return,
            r = listener.accept() => match r { Ok(v) => v, Err(_) => continue },
        };
        let (reg, tls) = (cfg.registry.clone(), tls.cloned());
        tokio::spawn(connection(reg, stream, peer, tls, cfg.log.clone()));
    }
}

async fn connection(
    reg: Registry,
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
    tls: Option<Tls>,
    log: Log,
) {
    let ip = peer.ip();
    let is_tls = tls.is_some();
    let service = service_fn(move |req| {
        let reg = reg.clone();
        async move {
            Ok::<_, std::convert::Infallible>(crate::proxy::handle(reg, req, ip, is_tls).await)
        }
    });
    let http = hyper::server::conn::http1::Builder::new();
    match tls {
        None => {
            let _ = http
                .serve_connection(TokioIo::new(stream), service)
                .with_upgrades()
                .await;
        }
        // Only this machine gets a handshake: a LAN peer learns nothing, not even which names are domains.
        Some(tls) if crate::on_this_machine(ip) => {
            let Ok(start) =
                LazyConfigAcceptor::new(rustls::server::Acceptor::default(), stream).await
            else {
                return;
            };
            let name = start.client_hello().server_name().map(str::to_owned);
            let certs = tls.certs.clone();
            let chosen = match name {
                Some(n) => {
                    let asked = n.clone();
                    match tokio::task::spawn_blocking(move || certs.choose(&asked)).await {
                        Ok(Ok(c)) => Some(c),
                        Ok(Err(why)) => {
                            // The client sees only an alert; the log says which name and why.
                            if tls.first_refusal(&n) {
                                log(refusal_line(&n, &why));
                            }
                            None
                        }
                        Err(_) => None,
                    }
                }
                None => None,
            };
            // No certificate: the handshake goes on to fail with an alert the client can read, as before.
            let mut cfg = (*tls.base).clone();
            cfg.cert_resolver = Arc::new(Chosen(chosen));
            if let Ok(s) = start.into_stream(Arc::new(cfg)).await {
                let _ = http
                    .serve_connection(TokioIo::new(s), service)
                    .with_upgrades()
                    .await;
            }
        }
        Some(_) => {}
    }
}
