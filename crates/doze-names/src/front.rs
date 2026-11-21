//! The registry's record of the shared sockets (who holds the resolver and the front door) and of each name's route.
//!
//! "Address in use" says nothing about who has it; telling a user a doze peer has the port when systemd-resolved
//! does sends them looking in the wrong place. So each holder is an entry, under a key no host can end in, and the
//! pid sweep frees it when the holder dies.

use crate::registry::{Entry, Registry};
use crate::{ENV_INGRESS, ENV_INGRESS_TLS, Error, Lease, normalize};

/// The front door, bound on the IPv4 wildcard explicitly: macOS binds a privileged port only on the wildcard
/// (measured: `:80` and `0.0.0.0:80` bind, `127.0.0.2:80` does not), and `:80` takes the IPv6 wildcard, which macOS
/// hands over even while another program holds IPv4 0.0.0.0:80, a socket no client reaches.
pub const INGRESS_BIND: &str = "0.0.0.0:80";
/// The https front door.
pub const INGRESS_TLS_BIND: &str = "0.0.0.0:443";

/// The bookkeeping keys: not valid in-zone names, so they never collide with one.
pub(crate) const RESOLVER_KEY: &str = "_resolver";
pub(crate) const INGRESS_KEY: &str = "_ingress";
pub(crate) const INGRESS_TLS_KEY: &str = "_ingress_tls";

fn env_or(var: &str, default: &str) -> String {
    std::env::var(var)
        .ok()
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| default.into())
}

/// Where the front door listens: [`INGRESS_BIND`], unless [`ENV_INGRESS`] moves it.
pub fn ingress_bind() -> String {
    env_or(ENV_INGRESS, INGRESS_BIND)
}

/// Where the https front door listens: [`INGRESS_TLS_BIND`], unless [`ENV_INGRESS_TLS`] moves it.
pub fn ingress_tls_bind() -> String {
    env_or(ENV_INGRESS_TLS, INGRESS_TLS_BIND)
}

/// The name part of a Host header (which may carry a port).
fn host_only(host: &str) -> &str {
    match host.rsplit_once(':') {
        Some((h, port)) if !h.is_empty() && port.parse::<u16>().is_ok() => h,
        _ => host,
    }
}

impl Registry {
    /// Records this process as the holder of a shared socket (`_resolver`, `_ingress`, `_ingress_tls`).
    pub(crate) fn claim_holder(&self, key: &str, addr: &str) -> Result<(), Error> {
        let entry = Entry {
            pid: i64::from(self.pid),
            owner: self.owner.clone(),
            target: addr.to_string(),
            ..Entry::default()
        };
        self.update(|m| {
            m.insert(key.to_string(), entry);
            Ok(())
        })
    }

    /// Drops the holder record under `key` if it is still the one this handle wrote with [`Registry::claim_holder`]:
    /// this pid, this owner, this address. A pid alone does not say whose record it is when two peers share a process
    /// (the tests run doze-kafka and doze-aws in one; an app that hosts more than one program would too): matched by
    /// pid only, a holder letting go could erase the record its successor had just written.
    pub(crate) fn release_holder(&self, key: &str, addr: &str) -> Result<(), Error> {
        let pid = i64::from(self.pid);
        self.update(|m| {
            if m.get(key)
                .is_some_and(|e| e.pid == pid && e.owner == self.owner && e.target == addr)
            {
                m.remove(key);
            }
            Ok(())
        })
    }

    fn holder(&self, key: &str) -> Option<Entry> {
        self.snapshot().remove(key)
    }

    /// The doze process serving the zone's DNS, if one is.
    pub fn resolver_holder(&self) -> Option<Entry> {
        self.holder(RESOLVER_KEY)
    }

    /// The doze process holding the front door, if one is: whether the program on :80 is ours, which a dial cannot
    /// say (anything else there answers doze names with its own site).
    pub fn ingress_holder(&self) -> Option<Entry> {
        self.holder(INGRESS_KEY)
    }

    /// The doze process holding the https front door, if one is.
    pub fn ingress_tls_holder(&self) -> Option<Entry> {
        self.holder(INGRESS_TLS_KEY)
    }

    /// Publishes a route for a name this process holds, without the lease in hand.
    pub fn route_for(&self, host: &str, target: &str) -> Result<(), Error> {
        let (host, pid) = (normalize(host), i64::from(self.pid));
        self.update(|m| match m.get_mut(&host) {
            Some(e) if e.pid == pid => {
                e.target = target.to_string();
                Ok(())
            }
            _ => Err(Error::NotHeld(host.clone())),
        })
    }

    /// The backend the front door should proxy a Host header to, if this machine fronts the name. The resolver's
    /// own walk: if DNS sends a browser to an address and the front door does not know the Host, the request is
    /// refused, which reads as the service being down.
    pub fn front_target(&self, host: &str) -> Option<String> {
        self.lookup(host_only(host))
            .map(|e| e.target)
            .filter(|t| !t.is_empty())
    }

    /// The URL to print for a host: port-less when a doze process holds the front door on 80 (asked of the
    /// registry, not the network: a dial only proves something answers, and that may be nginx), with the front
    /// door's port when it was moved, else the backend's port; `None` for a name with no route.
    pub fn url_for(&self, host: &str) -> Option<String> {
        let target = self
            .lookup(host)
            .map(|e| e.target)
            .filter(|t| !t.is_empty())?;
        if let Some(h) = self.ingress_holder() {
            return Some(match h.target.rsplit_once(':') {
                Some((_, port)) if port != "80" => format!("http://{host}:{port}"),
                _ => format!("http://{host}"),
            });
        }
        let (_, port) = target.rsplit_once(':')?;
        Some(format!("http://{host}:{port}"))
    }
}

impl Lease {
    /// Publishes where this name's traffic goes ("127.0.0.2:4566"). Until it has a route the front door does not
    /// answer for the name, so a service that wants only DNS leaves it unset.
    pub fn route(&self, target: &str) -> Result<(), Error> {
        self.reg.route_for(&self.name.host, target)
    }
}
