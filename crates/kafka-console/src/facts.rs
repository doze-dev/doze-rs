//! What the binary knows about how the broker was started and the console is served: the flags, as values.

use std::net::SocketAddr;

use crate::model::Access;
use crate::model::Chrome;

/// How the broker was started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// Where the Kafka listener is bound.
    pub listen: SocketAddr,
    /// What clients are told to reconnect to.
    pub advertised_host: String,
    pub advertised_port: u16,
    /// Whether the Kafka listener serves TLS.
    pub tls: bool,
    /// The SASL mechanisms offered, by name; empty: nobody is asked to log in.
    pub sasl: Vec<String>,
    /// The Kafka major version being stood for.
    pub kafka_major: u8,
    /// `--console-allow-host`.
    pub allow_hosts: Vec<String>,
    /// The console is reachable from other machines (it is bound beyond loopback): it has no login, and every page says so.
    pub unprotected: bool,
}

impl Facts {
    /// What a client is pointed at: the listener's address, or for a wildcard bind the advertised name.
    #[must_use]
    pub fn bootstrap(&self) -> String {
        if self.listen.ip().is_unspecified() {
            join(&self.advertised_host, self.listen.port())
        } else {
            self.listen.to_string()
        }
    }

    /// What Metadata tells clients to reconnect to.
    #[must_use]
    pub fn advertised(&self) -> String {
        join(&self.advertised_host, self.advertised_port)
    }

    /// The address a dial of the listener uses from this machine: a wildcard is reached through loopback.
    #[must_use]
    pub fn dial_listener(&self) -> String {
        match self.listen.ip() {
            ip if ip.is_unspecified() && ip.is_ipv6() => format!("[::1]:{}", self.listen.port()),
            ip if ip.is_unspecified() => format!("127.0.0.1:{}", self.listen.port()),
            _ => self.listen.to_string(),
        }
    }

    /// Whether only this machine can reach the listener.
    #[must_use]
    pub fn loopback(&self) -> bool {
        self.listen.ip().is_loopback()
    }
}

/// `host:port`, with brackets around an IPv6 literal.
fn join(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

/// What the security chip and the Access rows say, from the flags and what the broker holds now.
#[must_use]
pub fn access(facts: &Facts, acls: usize, default_allow: bool) -> Access {
    let sasl = !facts.sasl.is_empty();
    let scram = facts.sasl.iter().any(|m| m.starts_with("SCRAM"));
    let plain = facts.sasl.iter().any(|m| m == "PLAIN");
    let mut label = match (sasl, scram, plain) {
        (false, _, _) => "Open",
        (true, true, false) => "SASL SCRAM",
        (true, false, true) => "SASL PLAIN",
        (true, _, _) => "SASL",
    }
    .to_owned();
    let acls_on = acls > 0 || !default_allow;
    if acls_on {
        label.push_str(" · ACLs");
    }
    if facts.tls {
        label.push_str(" + TLS");
    }
    let sign_in = if sasl {
        "Sign-in required"
    } else {
        "No sign-in"
    };
    let rules = match (acls, default_allow) {
        (0, true) => "no ACLs".to_owned(),
        (0, false) => "ACLs deny by default".to_owned(),
        (n, _) => format!("{n} ACL{}", if n == 1 { "" } else { "s" }),
    };
    let wire = if facts.tls { "TLS" } else { "plain TCP" };
    Access {
        label,
        sasl,
        detail: format!("{sign_in}, {rules}, {wire}"),
    }
}

/// The top bar from the facts.
#[must_use]
pub fn chrome(facts: &Facts, access: &Access) -> Chrome {
    Chrome {
        address: facts.bootstrap(),
        profile: format!("Kafka {} profile", facts.kafka_major),
        security: access.label.clone(),
        locked: access.sasl || facts.tls,
        unprotected: facts.unprotected,
    }
}

#[cfg(test)]
mod tests;
