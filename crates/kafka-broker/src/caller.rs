//! Who is asking, and from where: what authorization is decided on.

use std::net::IpAddr;

use kafka_auth::Principal;
use kafka_auth::authz::Subject;

/// The principal a connection is authorized as, and the address it came from. Built by the server for each
/// connection (and again, with the new principal, when a login completes) and handed to every request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    principal: Principal,
    host: String,
}

impl Caller {
    /// `principal`, connected from `peer` (`None`: not over a network, as in a test, and then only the ACLs whose
    /// host is `*` can apply).
    #[must_use]
    pub fn new(principal: Principal, peer: Option<IpAddr>) -> Self {
        Self {
            principal,
            host: peer.map_or_else(String::new, host_text),
        }
    }

    /// A connection that never logged in: `User:ANONYMOUS`.
    #[must_use]
    pub fn anonymous(peer: Option<IpAddr>) -> Self {
        Self::new(Principal::anonymous(), peer)
    }

    /// The principal.
    #[must_use]
    pub fn principal(&self) -> &Principal {
        &self.principal
    }

    /// The address ACL hosts are compared with.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    pub(crate) fn subject(&self) -> Subject<'_> {
        Subject {
            principal: self.principal.as_str(),
            host: &self.host,
        }
    }
}

/// An address as Java's `InetAddress.getHostAddress` writes it, which is what Kafka compares an ACL's host with:
/// dotted decimal for IPv4 (an IPv4-mapped IPv6 address is an IPv4 one), and for IPv6 eight groups of hexadecimal
/// without leading zeros and without the `::` shortening (`0:0:0:0:0:0:0:1`, so an ACL written `::1` never
/// matches, in Kafka as here).
fn host_text(ip: IpAddr) -> String {
    match ip.to_canonical() {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => v6
            .segments()
            .iter()
            .map(|s| format!("{s:x}"))
            .collect::<Vec<_>>()
            .join(":"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_is_written_as_java_writes_it() {
        let text = |s: &str| host_text(s.parse().unwrap());
        assert_eq!(text("127.0.0.1"), "127.0.0.1");
        assert_eq!(text("::1"), "0:0:0:0:0:0:0:1");
        assert_eq!(text("fe80::1:2"), "fe80:0:0:0:0:0:1:2");
        assert_eq!(
            text("2001:db8::ff00:42:8329"),
            "2001:db8:0:0:0:ff00:42:8329"
        );
        assert_eq!(text("::ffff:10.1.2.3"), "10.1.2.3");
    }

    #[test]
    fn a_caller_without_an_address_has_a_host_nothing_but_star_matches() {
        let c = Caller::anonymous(None);
        assert_eq!(c.host(), "");
        assert_eq!(c.principal().as_str(), "User:ANONYMOUS");
        assert_eq!(
            Caller::anonymous(Some("10.0.0.7".parse().unwrap())).host(),
            "10.0.0.7"
        );
    }
}
