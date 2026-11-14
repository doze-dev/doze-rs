//! Linux: a route is a routing domain on the zone's link in systemd-resolved (`~dev.acme.com` on doze0), one setting
//! that holds the zone and every domain together. Machines without resolved have no route for a domain; the name works
//! with its port. The authority goes into the distribution's bundle: Debian's update-ca-certificates or Fedora's
//! update-ca-trust, whichever is there.

use super::output;
use crate::ca::Ca;
use crate::resolvconf::{Manager, detect};
use crate::{RESOLVER_IFACE, RESOLVER_IP, RESOLVER_PORT};
use std::path::Path;

const FEDORA_ANCHORS: &str = "/etc/pki/ca-trust/source/anchors";

/// Where the doze root goes in the distribution's bundle, and the command that rebuilds the bundle from it.
///
/// Debian's with `--fresh`: the file keeps its name across a rotation, and without it update-ca-certificates sees no
/// new file, rebuilds the bundle but leaves /etc/ssl/certs's hash link pointing at the old root's subject, so a client
/// that looks roots up by hash (`-CApath`) does not find the new one (seen in the Lima VM over Go's root; Go ran it
/// without).
pub(crate) fn ca_bundle_path() -> (&'static str, &'static str) {
    if Path::new(FEDORA_ANCHORS).is_dir() {
        (
            "/etc/pki/ca-trust/source/anchors/doze-local-ca.crt",
            "update-ca-trust",
        )
    } else {
        (
            "/usr/local/share/ca-certificates/doze-local-ca.crt",
            "update-ca-certificates --fresh",
        )
    }
}

/// The link's domain list in `resolvectl domain <link>` output (`Link 5 (doze0): ~doze ~dev.acme.com`), one token
/// each.
// fuzz:parser fuzz_setup
pub fn link_domains(out: &str) -> Vec<String> {
    out.split_once(':')
        .map(|(_, rest)| rest.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default()
}

pub(super) fn domain_routed(domain: &str) -> (bool, String) {
    if detect(Path::new("/etc/resolv.conf")) != Manager::Resolved {
        return (
            false,
            format!("{domain} needs systemd-resolved to be routed; use the name with its port"),
        );
    }
    let want = format!("~{domain}");
    let routed = output("resolvectl", &["domain", RESOLVER_IFACE])
        .is_some_and(|out| link_domains(&out).contains(&want));
    if routed {
        (
            true,
            format!("resolved sends {domain} to {RESOLVER_IP}:{RESOLVER_PORT}"),
        )
    } else {
        (
            false,
            format!("{domain} is not routed to the doze resolver"),
        )
    }
}

pub(super) fn ca_trusted(ca: &Ca) -> (bool, String) {
    let (path, _) = ca_bundle_path();
    match (std::fs::read(path), std::fs::read(ca.root_path())) {
        (Ok(have), Ok(want)) if have == want => {
            (true, format!("trusted in the system bundle · {path}"))
        }
        _ => (false, format!("not trusted · {}", ca.root_path().display())),
    }
}
