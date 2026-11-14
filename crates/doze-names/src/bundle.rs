//! The bundle: one PEM file holding the roots this machine trusts and the doze root, for the clients that take a file
//! in place of their trust store (`SSL_CERT_FILE`, `REQUESTS_CA_BUNDLE`, `NODE_EXTRA_CA_CERTS`). Those variables
//! replace a client's store rather than add to it, so pointing them at the doze root alone would cut a stack off from
//! every public https host; the bundle keeps them whole.

use crate::Error;
use crate::ca::{Ca, write_atomic};
use crate::trust_store::{CA_COMMON_NAME, platform_roots_pem};
use std::path::PathBuf;
use x509_cert::Certificate;
use x509_cert::der::DecodePem;
use x509_cert::ext::pkix::BasicConstraints;

const BUNDLE: &str = "bundle.pem";

/// The distributions' bundle files, for systems that do not say otherwise (Debian, Fedora, SUSE, Alpine and BSD).
const BUNDLE_FILES: [&str; 4] = [
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/ssl/ca-bundle.pem",
    "/etc/ssl/cert.pem",
];

/// Each PEM block in `text` (from its BEGIN line to the end of its END line), as it was written. Text outside blocks is
/// not a certificate and is left behind.
// fuzz:parser fuzz_setup
pub fn pem_blocks(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("-----BEGIN ") {
        let Some(tail) = rest.get(start..) else { break };
        let Some(end_at) = tail.find("-----END ") else {
            break;
        };
        let after_end = tail.get(end_at + 9..).unwrap_or_default();
        // The END line's closing dashes: "-----END CERTIFICATE-----".
        let Some(close) = after_end.find("-----") else {
            break;
        };
        let len = end_at + 9 + close + 5;
        let Some(block) = tail.get(..len) else { break };
        out.push(block);
        rest = tail.get(len..).unwrap_or_default();
    }
    out
}

/// The certificate in a PEM block, when it is one this crate can read.
fn certificate(block: &str) -> Option<Certificate> {
    if !block.starts_with("-----BEGIN CERTIFICATE-----") {
        return None;
    }
    Certificate::from_pem(block.as_bytes()).ok()
}

/// A certificate's common name.
pub(crate) fn common_name(cert: &Certificate) -> Option<String> {
    let cn = cert.tbs_certificate().subject().common_name().ok()??;
    Some(String::from(cn))
}

fn doze_root(block: &str) -> bool {
    certificate(block)
        .and_then(|c| common_name(&c))
        .is_some_and(|cn| cn.starts_with(CA_COMMON_NAME))
}

/// Whether a PEM block is an authority that is not doze's: worth asking the store whether it is a trusted anchor.
pub(crate) fn is_trusted_anchor_candidate(block: &str) -> bool {
    certificate(block).is_some_and(|c| {
        let ca = matches!(
            c.tbs_certificate().get_extension::<BasicConstraints>(),
            Ok(Some((_, bc))) if bc.ca
        );
        ca && !common_name(&c).is_some_and(|cn| cn.starts_with(CA_COMMON_NAME))
    })
}

/// A short stable tag for a block, for a temporary file's name.
pub(crate) fn short_hash(block: &str) -> String {
    format!("{:08x}", crate::pool::fnv1a32(block.as_bytes()))
}

/// `text` with every doze authority dropped, current or rotated out, and everything else kept as it was (a public root
/// this crate cannot parse is kept too: the system shipped it).
fn without_doze_roots(text: &str, out: &mut String) {
    for block in pem_blocks(text) {
        if !doze_root(block) {
            out.push_str(block);
            out.push('\n');
        }
    }
}

/// The machine's trusted roots as PEM: what the system says (macOS), else the distribution's bundle file.
fn system_roots_pem() -> String {
    let from_platform = platform_roots_pem();
    if !from_platform.is_empty() {
        return from_platform;
    }
    BUNDLE_FILES
        .iter()
        .find_map(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default()
}

fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

impl Ca {
    /// Where [`Ca::bundle`] writes.
    pub fn bundle_path(&self) -> PathBuf {
        self.dir.join(BUNDLE)
    }

    /// Writes `<ca>/bundle.pem` and returns its path: the roots this machine trusts, whatever the `SSL_CERT_FILE` this
    /// process inherited holds (a corporate bundle, say), and the doze root. An inherited `SSL_CERT_FILE` that is this
    /// bundle itself (a shell that ran `eval "$(doze env)"`) is not folded in, and no doze root but the current one is
    /// carried: a rotated-out root never rides along. Written under the authority's lock, so two daemons starting
    /// together do not race on it.
    pub fn bundle(&self) -> Result<PathBuf, Error> {
        let inherited = std::env::var_os("SSL_CERT_FILE").map(PathBuf::from);
        self.bundle_from(inherited.as_deref())
    }

    /// [`Ca::bundle`] with the inherited file given (tests cannot set the environment without `unsafe`).
    pub(crate) fn bundle_from(
        &self,
        inherited: Option<&std::path::Path>,
    ) -> Result<PathBuf, Error> {
        let _lock = doze_os::fs::lock(&self.dir.join(".lock"))?;
        let root = std::fs::read_to_string(self.root_path())?;
        let mut out = String::new();
        without_doze_roots(&system_roots_pem(), &mut out);
        if let Some(inherited) = inherited
            && !same_file(inherited, &self.bundle_path())
            && let Ok(extra) = std::fs::read_to_string(inherited)
        {
            without_doze_roots(&extra, &mut out);
        }
        out.push_str(&root);
        let path = self.bundle_path();
        write_atomic(&path, out.as_bytes(), false)?;
        Ok(path)
    }

    /// [`Ca::bundle`], unless a bundle at least as new as the root is there already: for a surface run often (`doze
    /// env` in a shell hook) that must point at a bundle but need not rewrite it each time.
    pub fn ensure_bundle(&self) -> Result<PathBuf, Error> {
        let modified = |p: PathBuf| std::fs::metadata(p).and_then(|m| Ok((m.modified()?, m.len())));
        if let (Ok((b, size)), Ok((r, _))) =
            (modified(self.bundle_path()), modified(self.root_path()))
            && b >= r
            && size > 0
        {
            return Ok(self.bundle_path());
        }
        self.bundle()
    }
}

#[cfg(test)]
#[path = "bundle_tests.rs"]
mod tests;
