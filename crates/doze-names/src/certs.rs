//! The certificate for each TLS handshake at the front door: the person's own files when they brought some, else a
//! leaf minted from the authority as it is on disk now. A name nobody registered gets none, so the handshake fails
//! rather than answering for a stranger.
//!
//! The holder of the https port may run for days while another process makes the authority or rotates it (`doze
//! trust --rotate`), so the root is looked up per handshake through a cache that rereads it when the file changed,
//! at most every two seconds; leaves of an old root go with it.

use crate::Registry;
use crate::ca::Ca;
use crate::domains::lookup_domain;
use rustls::crypto::ring::sign::any_supported_type;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivateSec1KeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// How often the root file is looked at.
pub const CA_CHECK_EVERY: Duration = Duration::from_secs(2);

#[derive(Debug, Default)]
struct Seen {
    at: Option<Instant>,
    /// The root file's modification time and size when loaded.
    id: Option<(SystemTime, u64)>,
    ca: Option<Arc<Ca>>,
}

/// The authority under a folder, as it is now.
#[derive(Debug)]
pub struct CaSource {
    dir: PathBuf,
    every: Duration,
    seen: Mutex<Seen>,
}

impl CaSource {
    pub fn new(dir: &Path, every: Duration) -> CaSource {
        CaSource {
            dir: dir.to_path_buf(),
            every,
            seen: Mutex::new(Seen::default()),
        }
    }

    /// The authority, reloaded when its root file changed; `None` when there is none (or it cannot be used).
    pub fn current(&self) -> Option<Arc<Ca>> {
        let mut seen = self.seen.lock().ok()?;
        if seen.at.is_some_and(|t| t.elapsed() < self.every) && !self.every.is_zero() {
            return seen.ca.clone();
        }
        seen.at = Some(Instant::now());
        let id = std::fs::metadata(self.dir.join("root.crt"))
            .ok()
            .map(|m| (m.modified().ok().unwrap_or(SystemTime::UNIX_EPOCH), m.len()));
        if id != seen.id || seen.ca.is_none() {
            seen.id = id;
            seen.ca = Ca::probe(&self.dir).ok().flatten().map(Arc::new);
        }
        seen.ca.clone()
    }
}

/// The front door's rustls certificate resolver.
#[derive(Debug)]
pub struct Certs {
    reg: Registry,
    ca: CaSource,
}

impl Certs {
    pub fn new(reg: Registry, ca: CaSource) -> Certs {
        Certs { reg, ca }
    }

    /// The certificate for `server_name`, or `None`.
    pub fn for_name(&self, server_name: &str) -> Option<Arc<CertifiedKey>> {
        let (entry, owner) = lookup_domain(&self.reg.snapshot(), server_name)?;
        if !entry.cert.is_empty() {
            // The person's own files, as they are now (a renewed file is picked up).
            return own_files(Path::new(&entry.cert), Path::new(&entry.key));
        }
        let leaf = self.ca.current()?.leaf(&owner).ok()?;
        let key = PrivateKeyDer::Sec1(PrivateSec1KeyDer::from(leaf.key_der.clone()));
        let signer = any_supported_type(&key).ok()?;
        Some(Arc::new(CertifiedKey::new(
            vec![CertificateDer::from(leaf.cert_der.clone())],
            signer,
        )))
    }
}

fn own_files(cert: &Path, key: &Path) -> Option<Arc<CertifiedKey>> {
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(cert)
        .ok()?
        .filter_map(Result::ok)
        .collect();
    let key = PrivateKeyDer::from_pem_file(key).ok()?;
    let signer = any_supported_type(&key).ok()?;
    (!chain.is_empty()).then(|| Arc::new(CertifiedKey::new(chain, signer)))
}

impl ResolvesServerCert for Certs {
    fn resolve(&self, hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.for_name(hello.server_name()?)
    }
}
