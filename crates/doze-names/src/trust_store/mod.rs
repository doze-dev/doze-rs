//! What the machine trusts and routes, asked without privilege: whether a domain is routed to the resolver, whether the
//! doze authority is trusted, which doze roots a store holds, and the system's own roots (for the bundle). The scripts
//! that change any of it are setup's (feature `setup`); these questions are asked by every surface that reports, and
//! by the CA's rotation, so they are here.
//!
//! One file per system, chosen at run time by `target_os` (as `pool.rs` chooses the address block). Each answers by
//! asking the system's own tool, the way the system itself decides: `security verify-cert` on macOS (a root can be in
//! the keychain and not trusted), `resolvectl` on Linux, PowerShell on Windows.

mod linux;
mod mac;
mod windows;

use crate::ca::Ca;

/// The common name every doze authority's starts with, how stores list it.
pub const CA_COMMON_NAME: &str = "doze local CA";

/// Whether `domain` is routed to the doze resolver, and a line saying how (or why not).
pub fn domain_routed(domain: &str) -> (bool, String) {
    if cfg!(target_os = "macos") {
        mac::domain_routed(domain)
    } else if cfg!(target_os = "linux") {
        linux::domain_routed(domain)
    } else if cfg!(target_os = "windows") {
        windows::domain_routed(domain)
    } else {
        (false, format!("{domain}: not on this system"))
    }
}

/// Whether something that is not doze routes `domain`: it is never written over or removed.
pub fn route_foreign(domain: &str) -> bool {
    if cfg!(target_os = "macos") {
        mac::route_foreign(domain)
    } else if cfg!(target_os = "windows") {
        windows::route_foreign(domain)
    } else {
        // Linux: a route is doze's own setting on doze's own link, so nothing there is anyone else's.
        false
    }
}

/// Whether the authority is trusted for TLS on this machine, and a line saying where.
pub fn ca_trusted(ca: &Ca) -> (bool, String) {
    if cfg!(target_os = "macos") {
        mac::ca_trusted(ca)
    } else if cfg!(target_os = "linux") {
        linux::ca_trusted(ca)
    } else if cfg!(target_os = "windows") {
        windows::ca_trusted(ca)
    } else {
        (false, "no trust store on this system".into())
    }
}

/// Whether a rotated-out root (by SHA-256 fingerprint) is still trusted: by evaluation when its file was kept, else by
/// its presence in the store.
pub fn fingerprint_trusted(ca: Option<&Ca>, fp: &str) -> bool {
    if !valid_fingerprint(fp) {
        return false;
    }
    if cfg!(target_os = "macos") {
        mac::fingerprint_trusted(ca, fp)
    } else if cfg!(target_os = "windows") {
        windows::fingerprint_trusted(fp)
    } else {
        // Linux: the bundle holds one doze root at a time (cp -f), so a rotated-out root is gone with the rotation.
        false
    }
}

/// The SHA-256 fingerprints of every doze authority the machine's store holds, trusted or not: what a removal must
/// cover when the files that made them are gone, and what rotation remembers.
pub fn trusted_doze_roots() -> Vec<String> {
    if cfg!(target_os = "macos") {
        mac::doze_roots()
    } else if cfg!(target_os = "windows") {
        windows::doze_roots()
    } else {
        Vec::new()
    }
}

/// The roots the machine trusts for TLS, as PEM, when the system has a way to say (macOS: Apple's curated file plus
/// any authority an administrator trusted); empty where the distribution's bundle file is the answer.
pub(crate) fn platform_roots_pem() -> String {
    if cfg!(target_os = "macos") {
        mac::roots_pem()
    } else {
        String::new()
    }
}

/// A SHA-256 in hex: the only thing from a file or a tool's output that may reach a script or a store lookup.
pub fn valid_fingerprint(fp: &str) -> bool {
    fp.len() == 64 && fp.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A program's standard output when it succeeded.
pub(crate) fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program)
        .args(args)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(feature = "setup")]
pub(crate) use linux::ca_bundle_path as linux_ca_bundle_path;
#[cfg(feature = "setup")]
pub(crate) use mac::{
    RESOLVER_DIR as MAC_RESOLVER_DIR, SYSTEM_KEYCHAIN, points_here as mac_points_here,
    resolver_port as mac_resolver_port,
};
#[cfg(feature = "setup")]
pub(crate) use windows::{ROUTE_COMMENT, rules as machine_nrpt_rules};

/// The parsers, for the fuzz target (crate `fuzzing`); not an API.
#[doc(hidden)]
pub use linux::link_domains;
#[doc(hidden)]
pub use mac::sha256_lines;
#[doc(hidden)]
pub use windows::{Rule as NrptRule, fields as nrpt_rules};
