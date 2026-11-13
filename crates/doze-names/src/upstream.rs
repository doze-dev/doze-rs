//! Forwarding. A query reaches this resolver for a domain only because the machine was told to send that domain here
//! (a route in /etc/resolver, or resolved's). When the stack that claimed it is down, the honest answer is whatever
//! the internet says: `dev.acme.com` may be a real host, and a laptop that cannot reach it whenever doze is not
//! running is a trap. So an unregistered name outside the zone goes to the machine's own upstream servers and their
//! answer is relayed. Nothing in the zone is ever forwarded: a typo there is NXDOMAIN.

use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;

const REFRESH: Duration = Duration::from_secs(60);
const TIMEOUT: Duration = Duration::from_secs(4);

/// Whether a server is ours, or a local stub that would send the query straight back here. 127.0.0.1:53 is a real
/// upstream when a local forwarder (dnscrypt, Pi-hole, dnsmasq) runs there: this resolver is on another port.
fn own(ip: &str) -> bool {
    matches!(ip, "127.0.0.53" | "127.0.0.54" | crate::RESOLVER_IP)
}

fn server(ip: &str) -> Option<SocketAddr> {
    let ip: IpAddr = ip.parse().ok()?;
    (!own(&ip.to_string())).then_some(SocketAddr::new(ip, 53))
}

/// The nameservers `scutil --dns` lists for every resolver block with no domain of its own: what the Mac asks for
/// everything else.
// fuzz:parser fuzz_text
pub fn scutil_servers(text: &str) -> Vec<SocketAddr> {
    let mut out: Vec<SocketAddr> = Vec::new();
    let mut scoped = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with("resolver #") {
            scoped = false;
        } else if line.starts_with("domain ") {
            scoped = true;
        } else if line.starts_with("nameserver[") && !scoped {
            let Some((_, v)) = line.split_once(':') else {
                continue;
            };
            if let Some(s) = server(v.trim()).filter(|s| !out.contains(s)) {
                out.push(s);
            }
        }
    }
    out
}

/// The `nameserver` lines of a resolv.conf.
// fuzz:parser fuzz_text
pub fn resolv_conf_servers(text: &str) -> Vec<SocketAddr> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            (f.next() == Some("nameserver")).then(|| f.next()).flatten()
        })
        .filter_map(server)
        .collect()
}

fn find() -> Vec<SocketAddr> {
    if cfg!(target_os = "macos") {
        let out = std::process::Command::new("/usr/sbin/scutil")
            .arg("--dns")
            .output();
        return out
            .map(|o| scutil_servers(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_default();
    }
    for path in ["/run/systemd/resolve/resolv.conf", "/etc/resolv.conf"] {
        let servers = std::fs::read_to_string(path)
            .map(|t| resolv_conf_servers(&t))
            .unwrap_or_default();
        if !servers.is_empty() {
            return servers;
        }
    }
    Vec::new()
}

/// The machine's upstream servers, found lazily and refreshed every minute; or a fixed list (tests).
#[derive(Debug)]
pub struct Upstreams {
    fixed: Option<Vec<SocketAddr>>,
    cache: Mutex<Option<(Instant, Vec<SocketAddr>)>>,
}

impl Upstreams {
    /// The machine's own.
    pub fn system() -> Upstreams {
        Upstreams {
            fixed: None,
            cache: Mutex::new(None),
        }
    }

    /// These servers and no others.
    pub fn fixed(addrs: Vec<SocketAddr>) -> Upstreams {
        Upstreams {
            fixed: Some(addrs),
            cache: Mutex::new(None),
        }
    }

    fn addrs(&self) -> Vec<SocketAddr> {
        if let Some(f) = &self.fixed {
            return f.clone();
        }
        let Ok(mut cache) = self.cache.lock() else {
            return find();
        };
        match cache.as_ref() {
            Some((at, addrs)) if at.elapsed() < REFRESH => addrs.clone(),
            _ => {
                let addrs = find();
                *cache = Some((Instant::now(), addrs.clone()));
                addrs
            }
        }
    }

    /// The first answer an upstream gives within four seconds, or `None`.
    pub(crate) async fn forward(&self, q: &[u8]) -> Option<Vec<u8>> {
        for addr in self.addrs() {
            let bind: SocketAddr = if addr.is_ipv4() {
                ([0, 0, 0, 0], 0).into()
            } else {
                ([0u16; 8], 0).into()
            };
            let Ok(sock) = UdpSocket::bind(bind).await else {
                continue;
            };
            if sock.connect(addr).await.is_err() || sock.send(q).await.is_err() {
                continue;
            }
            let mut buf = vec![0u8; 4096];
            if let Ok(Ok(n)) = tokio::time::timeout(TIMEOUT, sock.recv(&mut buf)).await
                && n >= 12
            {
                buf.truncate(n);
                return Some(buf);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstreams_skip_loopback_stubs_and_scoped_resolvers() {
        // Go: TestUpstreamsSkipLoopbackStubs.
        let conf = "# resolved\nnameserver 127.0.0.53\nnameserver 192.168.1.1\nnameserver 127.0.0.1\nsearch lan\n";
        let got: Vec<String> = resolv_conf_servers(conf)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(got, ["192.168.1.1:53", "127.0.0.1:53"]);
        let scutil = "resolver #1\n  nameserver[0] : 10.0.0.1\n  nameserver[1] : 10.0.0.1\nresolver #2\n  domain   : \
                      doze\n  nameserver[0] : 127.0.0.1\nresolver #3\n  nameserver[0] : 1.1.1.1\n";
        let got: Vec<String> = scutil_servers(scutil)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            got,
            ["10.0.0.1:53", "1.1.1.1:53"],
            "the doze-scoped resolver is not an upstream"
        );
    }
}
