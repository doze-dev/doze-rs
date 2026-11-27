//! The certificate for each TLS handshake at the front door: the person's own files when they brought some, else a
//! leaf minted from the authority as it is on disk now. A name nobody registered gets none, so the handshake fails
//! rather than answering for a stranger.
//!
//! Which names: a declared domain and what its wildcard covers, and a name in the zone the front door fronts (one
//! with a route). A registered name owns its subtree ([`Registry::lookup`]), so a name under it is answered too:
//! `abc.execute-api.us-east-1.files.shop.doze` under `files.shop.doze`, or `a.b.dev.acme.com` under
//! `*.dev.acme.com`. A certificate for the owner would fail the client's hostname check there, so such a name gets a
//! leaf for exactly itself ([`Ca::leaf_under`], bounded); the person's own files are served as they are.
//!
//! The holder of the https port may run for days while another process makes the authority or rotates it (`doze
//! trust --rotate`), so the root is looked up per handshake through a cache that rereads it when the file changed,
//! at most every two seconds; leaves of an old root go with it.

use crate::ca::Ca;
use crate::domains::{lookup_domain, route_domain, shared_suffix, wildcard};
use crate::registry::zone_owner;
use crate::{Registry, in_zone, normalize};
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

    /// Makes the authority when there is none (files under the doze home, no privilege), so a handshake gets a
    /// certificate a client can name the issuer of, rather than a bare alert. One that is there and cannot be used is
    /// an error, never replaced (`doze trust --rotate` does that).
    pub fn ensure(&self) -> Result<(), crate::Error> {
        if self.current().is_some() {
            return Ok(());
        }
        Ca::load_or_create(&self.dir)?;
        if let Ok(mut seen) = self.seen.lock() {
            seen.at = None;
        }
        Ok(())
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

/// Why a handshake gets no certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// No doze program fronts the name (not registered, or registered without a route): it is not ours to answer.
    NotFronted,
    /// There is no usable certificate authority, and none could be made (says why).
    NoAuthority(String),
    /// The person's own certificate files could not be read.
    OwnFiles(String),
    /// A leaf could not be minted.
    Mint(String),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::NotFronted => f.write_str("no doze program serves that name over http"),
            Refused::NoAuthority(why) => write!(f, "{why} (doze trust --rotate makes a new one)"),
            Refused::OwnFiles(cert) => write!(
                f,
                "the certificate {cert} the stack declares cannot be read"
            ),
            Refused::Mint(why) => write!(f, "a certificate could not be made: {why}"),
        }
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

    /// Where the authority comes from.
    pub fn ca(&self) -> &CaSource {
        &self.ca
    }

    /// The certificate for `server_name`, or `None`.
    pub fn for_name(&self, server_name: &str) -> Option<Arc<CertifiedKey>> {
        self.choose(server_name).ok()
    }

    /// The certificate for `server_name`, or why there is none: what the front door's log says when it refuses a
    /// handshake, since the client only sees a bare alert.
    pub fn choose(&self, server_name: &str) -> Result<Arc<CertifiedKey>, Refused> {
        let name = normalize(server_name);
        let snap = self.reg.snapshot();
        let owned = if in_zone(&name) {
            zone_owner(&snap, &name)
        } else {
            lookup_domain(&snap, &name)
        };
        // Only what the front door fronts: a name without a route answers nothing over https either (http's
        // `front_target`). And never under a wildcard over a shared suffix (`*.co.uk`), which a registry written by
        // an older program may hold: that would mint for strangers' names.
        let (entry, owner) = owned
            .filter(|(e, o)| {
                !e.target.is_empty() && !(wildcard(o) && shared_suffix(&route_domain(o)))
            })
            .ok_or(Refused::NotFronted)?;
        if !entry.cert.is_empty() {
            // The person's own files, as they are now (a renewed file is picked up).
            return own_files(Path::new(&entry.cert), Path::new(&entry.key))
                .ok_or_else(|| Refused::OwnFiles(entry.cert.clone()));
        }
        // The first handshake that needs the authority makes it (files only, no privilege): https answers every name
        // the front door fronts (PLAN, "TLS and proxying"), and a machine that does not trust it yet gets an "unknown
        // issuer: doze local CA" a person can act on, rather than a bare alert.
        let ca = match self.ca.current() {
            Some(ca) => ca,
            None => {
                self.ca.ensure().map_err(|e| {
                    Refused::NoAuthority(format!(
                        "the certificate authority in {} cannot be used: {e}",
                        self.ca.dir.display()
                    ))
                })?;
                self.ca.current().ok_or_else(|| {
                    Refused::NoAuthority(format!(
                        "no certificate authority in {}",
                        self.ca.dir.display()
                    ))
                })?
            }
        };
        let leaf = if covers(&owner, &name) {
            ca.leaf(&owner)
        } else {
            ca.leaf_under(&name)
        }
        .map_err(|e| Refused::Mint(e.to_string()))?;
        let key = PrivateKeyDer::Sec1(PrivateSec1KeyDer::from(leaf.key_der.clone()));
        let signer = any_supported_type(&key).map_err(|e| Refused::Mint(e.to_string()))?;
        Ok(Arc::new(CertifiedKey::new(
            vec![CertificateDer::from(leaf.cert_der.clone())],
            signer,
        )))
    }
}

/// Whether the owner's own certificate carries `name`: it is the owner, or one label under a wildcard owner (whose
/// certificate also names what the wildcard is under).
fn covers(owner: &str, name: &str) -> bool {
    if owner == name {
        return true;
    }
    let Some(base) = owner.strip_prefix("*.") else {
        return false;
    };
    name == base
        || name
            .split_once('.')
            .is_some_and(|(first, rest)| rest == base && first != "*")
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

/// A certificate already chosen for one handshake (or none, so it fails): what the front door hands rustls once
/// [`Certs::for_name`] has run off the async workers.
#[derive(Debug)]
pub struct Chosen(pub Option<Arc<CertifiedKey>>);

impl ResolvesServerCert for Chosen {
    fn resolve(&self, _: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.0.clone()
    }
}

impl ResolvesServerCert for Certs {
    fn resolve(&self, hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.for_name(hello.server_name()?)
    }
}

/// A certificate and its key from PEM files, as DER (the first certificate, the key in the form the file has it): for
/// a host that hands them to another process, a module terminating TLS for its clients.
pub fn pem_pair_der(cert: &Path, key: &Path) -> Result<(Vec<u8>, Vec<u8>), String> {
    let first = CertificateDer::pem_file_iter(cert)
        .map_err(|e| format!("{}: {e}", cert.display()))?
        .next()
        .ok_or_else(|| format!("{}: no certificate in it", cert.display()))?
        .map_err(|e| format!("{}: {e}", cert.display()))?;
    let k = PrivateKeyDer::from_pem_file(key).map_err(|e| format!("{}: {e}", key.display()))?;
    any_supported_type(&k).map_err(|e| format!("{}: {e}", key.display()))?;
    Ok((first.to_vec(), k.secret_der().to_vec()))
}
