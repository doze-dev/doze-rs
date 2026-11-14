//! macOS: a route is a file in /etc/resolver named after the domain, which the system consults for that name and
//! everything under it (resolver(5)), the way /etc/resolver/doze routes the zone. The authority is a trusted root in
//! the System keychain; Safari and Chrome read it, and Firefox 120 and later do too.
//!
//! Trust is checked the way the keychain checks it, with verify-cert, not by whether the certificate is present: a root
//! can be in the keychain and not trusted, or trusted and then not. The basic policy is the one a root passes when
//! trusted (the ssl policy demands transparency logs of a root given as a leaf, so it fails for every root, public ones
//! included).

use super::{CA_COMMON_NAME, output, valid_fingerprint};
use crate::ca::Ca;
use std::path::Path;

pub(crate) const SYSTEM_KEYCHAIN: &str = "/Library/Keychains/System.keychain";
pub(crate) const RESOLVER_DIR: &str = "/etc/resolver";

/// The port the zone's resolver listens on, as a resolver file spells it.
pub(crate) fn resolver_port() -> String {
    let addr = crate::resolver_addr();
    addr.rsplit_once(':')
        .map(|(_, p)| p.to_string())
        .unwrap_or(addr)
}

/// Whether a resolver file's text points at this zone's resolver.
pub(crate) fn points_here(text: &str) -> bool {
    text.contains("127.0.0.1") && text.contains(&resolver_port())
}

/// A domain's resolver file: ours (points at this resolver), someone else's, or none.
fn route_file(domain: &str) -> (bool, bool) {
    match std::fs::read_to_string(Path::new(RESOLVER_DIR).join(domain)) {
        Ok(text) => (points_here(&text), true),
        Err(_) => (false, false),
    }
}

pub(super) fn domain_routed(domain: &str) -> (bool, String) {
    let file = format!("{RESOLVER_DIR}/{domain}");
    match route_file(domain) {
        (true, _) => (true, format!("{file} → {}", crate::resolver_addr())),
        (false, true) => (
            false,
            format!(
                "{file} is someone else's and points elsewhere; doze leaves it alone — remove it to let doze route \
                 {domain}"
            ),
        ),
        (false, false) => (
            false,
            format!("{file} missing — {domain} will not resolve here"),
        ),
    }
}

pub(super) fn route_foreign(domain: &str) -> bool {
    matches!(route_file(domain), (false, true))
}

/// Whether the keychain trusts a root certificate file as an anchor.
pub(crate) fn root_trusted(cert_file: &Path) -> bool {
    std::process::Command::new("security")
        .args(["verify-cert", "-L", "-l", "-q", "-p", "basic", "-c"])
        .arg(cert_file)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

pub(super) fn ca_trusted(ca: &Ca) -> (bool, String) {
    if root_trusted(&ca.root_path()) {
        return (
            true,
            format!(
                "trusted in the System keychain · {} · expires {}",
                ca.common_name(),
                ca.expires()
            ),
        );
    }
    (false, format!("not trusted · {}", ca.root_path().display()))
}

pub(super) fn fingerprint_trusted(ca: Option<&Ca>, fp: &str) -> bool {
    if let Some(file) = ca.map(|c| c.previous_path(fp)).filter(|f| f.exists()) {
        return root_trusted(&file);
    }
    doze_roots()
        .iter()
        .any(|have| have.eq_ignore_ascii_case(fp))
}

pub(super) fn doze_roots() -> Vec<String> {
    output(
        "security",
        &[
            "find-certificate",
            "-a",
            "-Z",
            "-c",
            CA_COMMON_NAME,
            SYSTEM_KEYCHAIN,
        ],
    )
    .map(|out| sha256_lines(&out))
    .unwrap_or_default()
}

/// The SHA-256 fingerprints in `security find-certificate -Z` output, lowercase, each once.
// fuzz:parser fuzz_setup
pub fn sha256_lines(out: &str) -> Vec<String> {
    let mut fps: Vec<String> = Vec::new();
    for line in out.lines() {
        if let Some((_, v)) = line.trim().split_once("SHA-256 hash:") {
            let fp = v.trim().to_ascii_lowercase();
            if valid_fingerprint(&fp) && !fps.contains(&fp) {
                fps.push(fp);
            }
        }
    }
    fps
}

/// Apple's curated file of the system roots trusted for TLS (the keychain of system roots holds more, some distrusted),
/// plus any authority an administrator put in the System keychain and trusted: a corporate root. A certificate merely
/// present there, such as the machine's own identities, is not an anchor and is left out, and so is doze's own.
pub(super) fn roots_pem() -> String {
    let mut out = std::fs::read_to_string("/etc/ssl/cert.pem")
        .ok()
        .or_else(|| {
            output(
                "security",
                &[
                    "find-certificate",
                    "-a",
                    "-p",
                    "/System/Library/Keychains/SystemRootCertificates.keychain",
                ],
            )
        })
        .unwrap_or_default();
    if let Some(admin) = output(
        "security",
        &["find-certificate", "-a", "-p", SYSTEM_KEYCHAIN],
    ) {
        for block in crate::bundle::pem_blocks(&admin) {
            if crate::bundle::is_trusted_anchor_candidate(block) && anchor_trusted(block) {
                out.push('\n');
                out.push_str(block);
            }
        }
    }
    out
}

/// Asks the keychain about one PEM certificate, through a file of its own (verify-cert reads files).
fn anchor_trusted(block: &str) -> bool {
    let dir = std::env::temp_dir();
    let file = dir.join(format!(
        "doze-anchor-{}-{}.pem",
        std::process::id(),
        crate::bundle::short_hash(block)
    ));
    if std::fs::write(&file, block).is_err() {
        return false;
    }
    let trusted = root_trusted(&file);
    let _ = std::fs::remove_file(&file);
    trusted
}
