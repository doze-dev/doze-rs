//! `.doze`, the local naming zone shared by every doze program: the doze CLI, doze-aws and doze-kafka
//! (docs/port/names/PLAN.md). A port of the Go package `doze-names`, which is its specification: the comments that
//! carry a measured reason say so and keep it.
//!
//! No program owns the zone. Each writes its names into a registry under the shared home, and whichever binds the
//! resolver first answers for all of them; if it exits, the next takes over. Install order does not matter.
//!
//! Two tiers of name. An apex name (`aws.doze`, `kafka.doze`) means "the one on this machine": it needs no stack,
//! which is what makes it right for a standalone doze-aws, and it is first-come; a second claimant is told who holds
//! it. A qualified name (`<service>.<stack>.doze`) belongs to a stack and is never contested, so a stack that loses
//! the apex race still has a working address.
//!
//! Every name resolves to an address of its own ([`pool`]), which is what lets each service hold its canonical port:
//! every Kafka on 9092, every local AWS on 80.

pub mod ca;
mod ca_cert;
mod ca_leaf;
pub mod certs;
pub mod claims;
pub mod domains;
pub mod error;
pub mod front;
pub mod front_door;
pub mod listen;
pub mod nat;
#[cfg(feature = "netd")]
pub mod netd;
pub mod pool;
pub mod ports;
mod proxy;
pub mod registry;
pub mod resolver;
pub mod table;
pub mod upstream;
pub(crate) mod wire;

pub use claims::{Lease, StackLease};
pub use error::Error;
pub use listen::{Forwarding, Listener};
pub use pool::Net;
pub use registry::{Entry, Registry};

use std::path::PathBuf;

/// The private top-level domain every doze name lives under.
pub const SUFFIX: &str = "doze";

/// Where the resolver listens on Linux: deliberately not a loopback address. systemd-resolved refuses loopback
/// addresses as DNS servers ("Invalid DNS server address") and the loopback interface too, so setup gives the
/// resolver an address of its own on a dummy interface, as Tailscale does with 100.100.100.100. 192.0.2.0/24 is
/// TEST-NET-1 (RFC 5737), never on a real network.
pub const RESOLVER_IP: &str = "192.0.2.53";
/// The dummy interface that carries [`RESOLVER_IP`] on Linux.
pub const RESOLVER_IFACE: &str = "doze0";
/// The resolver's port: deliberately not 53. systemd-resolved holds 53 on the wildcard (measured on Ubuntu with
/// systemd 255: 0.0.0.0:53, 127.0.0.53:53 and 127.0.0.54:53 all in use), and systemd-resolved (since v249,
/// `DNS=addr:port`) and dnsmasq (`server=/zone/addr#port`) both take a port.
pub const RESOLVER_PORT: u16 = 5323;

/// Moves the zone's DNS server, for a zone that must not touch the machine's real one (a test, two zones side by
/// side). With [`ENV_HOME`] and the ingress variables it makes a zone private; every program in it must be given the
/// same values.
pub const ENV_RESOLVER: &str = "DOZE_ZONE_RESOLVER";
/// Moves the http front door, e.g. `127.0.0.1:18080`.
pub const ENV_INGRESS: &str = "DOZE_ZONE_INGRESS";
/// Moves the https front door, e.g. `127.0.0.1:18443`.
pub const ENV_INGRESS_TLS: &str = "DOZE_ZONE_INGRESS_TLS";
/// The shared doze home, as doze core names it.
pub const ENV_HOME: &str = "DOZE_HOME";
/// Turns the zone off for every doze program at once: `DOZE_ZONE=off` (or 0, false, no).
pub const ENV_ZONE: &str = "DOZE_ZONE";

/// Which tier a name is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    /// `aws.doze`: machine-wide, first-come.
    Apex,
    /// `cloud.shop.doze`: a stack's own.
    Qualified,
    /// `dev.acme.com`: a name outside the zone an app declared ([`domains`]).
    Domain,
}

impl Tier {
    /// The spelling in the registry file.
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Apex => "apex",
            Tier::Qualified => "qualified",
            Tier::Domain => "domain",
        }
    }

    /// The tier the registry file spells `s`; an unknown spelling is a qualified name, the commonest.
    pub fn parse(s: &str) -> Tier {
        match s {
            "apex" => Tier::Apex,
            "domain" => Tier::Domain,
            _ => Tier::Qualified,
        }
    }
}

/// A host in the zone (or a declared domain), and its tier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Name {
    pub host: String,
    pub tier: Tier,
}

impl Name {
    /// The machine-wide name for a service: `apex("aws")` is `aws.doze`.
    pub fn apex(service: &str) -> Name {
        Name {
            host: format!("{}.{SUFFIX}", label(service)),
            tier: Tier::Apex,
        }
    }

    /// A stack service's name: `qualified("cloud", "shop")` is `cloud.shop.doze`.
    pub fn qualified(service: &str, stack: &str) -> Name {
        Name {
            host: format!("{}.{}.{SUFFIX}", label(service), label(stack)),
            tier: Tier::Qualified,
        }
    }

    /// The first label: the key into the apex table.
    pub(crate) fn service(&self) -> &str {
        self.host.split('.').next().unwrap_or_default()
    }
}

impl std::fmt::Display for Name {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.host)
    }
}

/// Whether a host belongs to the zone.
pub fn in_zone(host: &str) -> bool {
    let h = normalize(host);
    h == SUFFIX || h.ends_with(&format!(".{SUFFIX}"))
}

/// One DNS label, sanitised: lowercase, `[a-z0-9-]`, no leading or trailing dash. The same rule as doze core's, so a
/// name means the same thing in both.
pub fn label(s: &str) -> String {
    let mapped: String = s
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                c
            } else {
                '-'
            }
        })
        .collect();
    mapped.trim_matches('-').to_string()
}

/// A host as names are compared: no trailing dots, ASCII lowercase (how a resolver receives names).
pub(crate) fn normalize(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
}

/// The doze home every program shares, as doze-os resolves it (`$DOZE_HOME`, else the platform's place). The programs
/// must agree on it or they will not see each other's names, which is why it is decided in one place.
pub fn home() -> PathBuf {
    doze_os::home::doze_home().unwrap_or_else(|_| PathBuf::from(format!(".{SUFFIX}")))
}

/// Whether the zone has been switched off with [`ENV_ZONE`].
pub fn disabled() -> bool {
    let v = std::env::var(ENV_ZONE).unwrap_or_default();
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "off" | "0" | "false" | "no"
    )
}

/// Where the resolver listens: [`ENV_RESOLVER`] if set; on Linux [`RESOLVER_IP`] (systemd-resolved will not take a
/// loopback server); elsewhere 127.0.0.1. Both on [`RESOLVER_PORT`].
pub fn resolver_addr() -> String {
    if let Ok(a) = std::env::var(ENV_RESOLVER)
        && !a.trim().is_empty()
    {
        return a.trim().to_string();
    }
    let ip = if cfg!(target_os = "linux") {
        RESOLVER_IP
    } else {
        "127.0.0.1"
    };
    format!("{ip}:{RESOLVER_PORT}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_labels_as_go_writes_them() {
        assert_eq!(Name::apex("aws").host, "aws.doze");
        assert_eq!(
            Name::qualified("Cloud API", "Shop_1").host,
            "cloud-api.shop-1.doze"
        );
        assert_eq!(label("--Web--"), "web");
        assert!(in_zone("AWS.Doze."));
        assert!(in_zone("doze"));
        assert!(!in_zone("aws.doze.evil.com"));
        assert!(!in_zone("dozer"));
    }
}

/// The parsers, for the fuzz targets (crates/doze-names/fuzz); not an API.
#[doc(hidden)]
pub mod fuzzing {
    pub use crate::upstream::{resolv_conf_servers, scutil_servers};
    pub use crate::wire::{answer, cap_ttl};
}

/// Whether an address is this machine talking to itself: loopback, or on a Mac one of the zone's translated addresses
/// (routed to this machine's own interface and nowhere else). The front door binds the wildcard, so it is reachable
/// from the network the machine is on, and the services behind it are development fixtures with no authentication:
/// only this machine is answered.
pub fn on_this_machine(ip: std::net::IpAddr) -> bool {
    let net = Net::native();
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_loopback() || (net.virtual_ && net.offset(v4).is_some()),
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
    }
}
