//! Your own domain. A stack can give an app a name outside the zone (`dev.acme.com`, or every name under it),
//! because sign-in providers send a browser back only to a name under a public top-level domain, over https. It is
//! an entry in the same registry, with the same liveness and the app's address ([`crate::Lease::twin`]); what
//! differs is how the machine is told to ask this resolver for it (a per-domain route) and that the front door
//! serves it over TLS.

use crate::registry::{Entry, Registry};
use crate::{Error, Lease, Tier, in_zone, label, normalize};
use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

/// Whether `host` can be an app's domain: lowercase DNS labels, at least two, optionally one leading wildcard label;
/// not in the zone, not localhost, not an address.
pub fn valid_domain(host: &str) -> Result<(), String> {
    let h = normalize(host);
    if h.is_empty() {
        return Err("a domain cannot be empty".into());
    }
    if h.parse::<IpAddr>().is_ok() {
        return Err(format!("{host:?} is an address, not a domain"));
    }
    if in_zone(&h) {
        return Err(format!(
            "{host:?} is a doze name already; domains are for names outside .{}",
            crate::SUFFIX
        ));
    }
    if h == "localhost" || h.ends_with(".localhost") {
        return Err(format!(
            "{host:?}: browsers and sign-in providers treat localhost specially; use a real domain"
        ));
    }
    let labels: Vec<&str> = h.split('.').collect();
    if labels.len() < 2 {
        return Err(format!(
            "{host:?} has no top-level domain; a domain needs at least two labels"
        ));
    }
    for (i, l) in labels.iter().enumerate() {
        if *l == "*" {
            if i != 0 {
                return Err(format!("{host:?}: a wildcard can only be the first label"));
            }
            continue;
        }
        if l.is_empty() || *l != label(l) || l.len() > 63 {
            return Err(format!(
                "{host:?}: {l:?} is not a DNS label (lowercase letters, digits and hyphens)"
            ));
        }
    }
    if labels.first() == Some(&"*") && labels.len() < 3 {
        return Err(format!("{host:?}: a wildcard needs a domain under it"));
    }
    Ok(())
}

/// Whether a domain stands for every name under it.
pub fn wildcard(host: &str) -> bool {
    normalize(host).starts_with("*.")
}

/// The name a machine route is installed for: the domain, or what a wildcard is under (`/etc/resolver/<this>` covers
/// it and everything beneath).
pub fn route_domain(host: &str) -> String {
    let h = normalize(host);
    h.strip_prefix("*.").map(String::from).unwrap_or(h)
}

/// The domain entry owning a host outside the zone, and the registered name it was found under: the exact name,
/// else a wildcard over it. A wildcard owns the name under it too (`*.dev.acme.com` serves `dev.acme.com`, as its
/// certificate carries), and every depth beneath for resolving and routing; its certificate covers one level, as
/// certificates do, so `a.b.dev.acme.com` resolves but fails the handshake.
pub(crate) fn lookup_domain(m: &BTreeMap<String, Entry>, host: &str) -> Option<(Entry, String)> {
    let domain = |key: &str| {
        m.get(key)
            .filter(|e| e.tier == Tier::Domain.as_str())
            .cloned()
    };
    let mut h = normalize(host);
    if let Some(e) = domain(&h) {
        return Some((e, h));
    }
    let star = format!("*.{h}");
    if let Some(e) = domain(&star) {
        return Some((e, star));
    }
    loop {
        let (_, rest) = h.split_once('.')?;
        if !rest.contains('.') {
            return None;
        }
        let star = format!("*.{rest}");
        if let Some(e) = domain(&star) {
            return Some((e, star));
        }
        h = rest.to_string();
    }
}

/// Domains folded onto what their wildcards are under, without repeats: one machine route each, sorted.
pub fn route_domains(domains: &[String]) -> Vec<String> {
    let set: BTreeSet<String> = domains.iter().map(|d| route_domain(d)).collect();
    set.into_iter().collect()
}

impl Registry {
    /// Whether a Host header names a domain this machine serves (the front door redirects those to https).
    pub fn is_domain(&self, host: &str) -> bool {
        let h = host.rsplit_once(':').map_or(host, |(h, _)| h);
        !in_zone(h) && lookup_domain(&self.snapshot(), h).is_some()
    }

    /// The domains registered on this machine, sorted.
    pub fn domains(&self) -> Vec<String> {
        let snap = self.snapshot();
        snap.into_iter()
            .filter(|(_, e)| e.tier == Tier::Domain.as_str())
            .map(|(h, _)| h)
            .collect()
    }
}

impl Lease {
    /// Serves this domain with the person's own certificate and key files instead of one minted from the doze CA.
    /// The files are read at each handshake that needs them, so a renewed file is picked up.
    pub fn certificate(&self, cert_file: &str, key_file: &str) -> Result<(), Error> {
        let pid = i64::from(self.reg.pid);
        let host = self.name.host.clone();
        self.reg.update(|m| match m.get_mut(&host) {
            Some(e) if e.pid == pid => {
                e.cert = cert_file.to_string();
                e.key = key_file.to_string();
                Ok(())
            }
            _ => Err(Error::NotHeld(host.clone())),
        })
    }
}
