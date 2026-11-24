//! Client TLS at the front (Go's `proxy.BuildServerTLS` and the `tls { }` block): the certificate a module that takes
//! its own connections (`wire.accept`, the postgres filter) terminates its clients' TLS with, and whether a plain
//! client is refused. The proxy-to-engine hop stays plain.
//!
//! The person's `cert` and `key` files when the block names them; else a certificate minted from the machine's doze
//! authority (Go made a self-signed one per project, which no client could verify): for `*.<stack>.doze` with names
//! on, for `localhost` without. `doze trust` puts the authority in the machine's store, so `sslmode=verify-full`
//! passes.
//!
//! The daemon runs for weeks, so the pair is not taken once at load: each `wire.accept` asks for it as it is now. The
//! authority is followed the way the front door's [`doze_names::certs`] follows it (the root file looked at at most
//! every [`CA_CHECK_EVERY`], reloaded when it changed, so `doze trust --rotate` is picked up), and its leaf is the
//! authority's own, renewed a week before it lapses. The person's files are read again when either one's modification
//! time or size changed (a renewal). What cannot be loaded at start stops the stack, as Go's did; a later reload that
//! fails keeps serving the last good pair, said once in the log, so a half-written renewal breaks no connection.

use doze_names::ca::Ca;
pub use doze_names::certs::CA_CHECK_EVERY;
use doze_names::certs::CaSource;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

/// A certificate and its key, DER.
pub type Pair = Arc<(Vec<u8>, Vec<u8>)>;

/// What a module terminating TLS is handed: the pair as it is now, and whether a plain client is refused. Cloning
/// shares the source.
#[derive(Debug, Clone)]
pub struct WireTls(Arc<Inner>);

#[derive(Debug)]
struct Inner {
    from: Source,
    required: bool,
    every: Duration,
    now: Mutex<Now>,
}

#[derive(Debug)]
enum Source {
    /// A leaf for `host` from the authority under a folder, as it is on disk now.
    Authority { ca: CaSource, host: String },
    /// The person's own files.
    Files { cert: PathBuf, key: PathBuf },
}

#[derive(Debug)]
struct Now {
    /// The last good pair.
    pair: Pair,
    /// When the person's files were last looked at, and what they were then.
    checked: Instant,
    seen: (FileId, FileId),
    /// A reload failed and was said; quiet until one succeeds again.
    failing: bool,
}

/// A file's modification time and size; `None` when it cannot be looked at.
type FileId = Option<(SystemTime, u64)>;

fn file_id(p: &Path) -> FileId {
    let m = std::fs::metadata(p).ok()?;
    Some((m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()))
}

impl WireTls {
    /// A leaf for `host` minted from the authority under `ca_dir` (made there when there is none); its root is looked
    /// at at most every `every`. An error when no leaf can be had now.
    pub fn from_authority(
        ca_dir: &Path,
        host: &str,
        required: bool,
        every: Duration,
    ) -> Result<WireTls, String> {
        let first = Ca::load_or_create(ca_dir)
            .and_then(|ca| ca.leaf(host))
            .map_err(|e| format!("configuring TLS: {e}"))?;
        let pair = Arc::new((first.cert_der.clone(), first.key_der.clone()));
        let from = Source::Authority {
            ca: CaSource::new(ca_dir, every),
            host: host.to_owned(),
        };
        Ok(WireTls::new(from, pair, (None, None), required, every))
    }

    /// The person's certificate and key files, read again when they change, looked at at most every `every`. An error
    /// when they cannot be loaded now.
    pub fn from_files(
        cert: &Path,
        key: &Path,
        required: bool,
        every: Duration,
    ) -> Result<WireTls, String> {
        let seen = (file_id(cert), file_id(key));
        let pair = load_files(cert, key)
            .map_err(|e| format!("configuring TLS: loading the keypair: {e}"))?;
        let from = Source::Files {
            cert: cert.to_path_buf(),
            key: key.to_path_buf(),
        };
        Ok(WireTls::new(from, Arc::new(pair), seen, required, every))
    }

    fn new(
        from: Source,
        pair: Pair,
        seen: (FileId, FileId),
        required: bool,
        every: Duration,
    ) -> WireTls {
        WireTls(Arc::new(Inner {
            from,
            required,
            every,
            now: Mutex::new(Now {
                pair,
                checked: Instant::now(),
                seen,
                failing: false,
            }),
        }))
    }

    /// A plain client is refused.
    pub fn required(&self) -> bool {
        self.0.required
    }

    /// The certificate and key to hand the next connection: current, or the last good pair when a reload failed.
    pub fn pair(&self) -> Pair {
        let inner = &*self.0;
        let mut now = inner.now.lock().unwrap_or_else(PoisonError::into_inner);
        let fresh = match &inner.from {
            Source::Authority { ca, host } => match ca.current() {
                Some(ca) => ca
                    .leaf(host)
                    .map(|l| Some((l.cert_der.clone(), l.key_der.clone())))
                    .map_err(|e| e.to_string()),
                None => Err(format!(
                    "no usable doze authority (`doze trust --rotate` makes one); the certificate for {host} is not renewed"
                )),
            },
            Source::Files { cert, key } => {
                if now.checked.elapsed() < inner.every && !inner.every.is_zero() {
                    Ok(None)
                } else {
                    now.checked = Instant::now();
                    let seen = (file_id(cert), file_id(key));
                    if seen == now.seen {
                        Ok(None)
                    } else {
                        // A failure is not retried until the files change again: a writer that is not done yet
                        // changes them once more when it is.
                        now.seen = seen;
                        load_files(cert, key).map(Some).map_err(|e| {
                            format!(
                                "reloading the keypair: {e}; the last one loaded is still served"
                            )
                        })
                    }
                }
            }
        };
        match fresh {
            Ok(Some(p)) => {
                if *now.pair != p {
                    now.pair = Arc::new(p);
                }
                now.failing = false;
            }
            Ok(None) => {}
            Err(e) => {
                if !now.failing {
                    tracing::warn!("client TLS: {e}");
                    now.failing = true;
                }
            }
        }
        now.pair.clone()
    }
}

/// The person's pair, whose key must be the certificate's: a renewal caught between its two writes is not served.
fn load_files(cert: &Path, key: &Path) -> Result<(Vec<u8>, Vec<u8>), String> {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let (c, k) = doze_names::certs::pem_pair_der(cert, key)?;
    let der = PrivateKeyDer::try_from(k.clone()).map_err(|e| format!("{}: {e}", key.display()))?;
    let signer = rustls::crypto::ring::sign::any_supported_type(&der)
        .map_err(|e| format!("{}: {e}", key.display()))?;
    rustls::sign::CertifiedKey::new(vec![CertificateDer::from(c.clone())], signer)
        .keys_match()
        .map_err(|e| {
            format!(
                "{} and {}: the key is not the certificate's ({e})",
                cert.display(),
                key.display()
            )
        })?;
    Ok((c, k))
}

/// The stack's TLS, if its doze.hcl has a `tls` block; an error says why it cannot be had (Go refused to start).
pub fn for_config(cfg: &doze_config::Config) -> Result<Option<WireTls>, String> {
    let t = &cfg.tls;
    if !t.enabled {
        return Ok(None);
    }
    let resolve = |p: &str| {
        let p = Path::new(p);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            Path::new(&cfg.dir).join(p)
        }
    };
    let tls = if t.cert.is_empty() {
        let host = if cfg.defaults.names {
            format!(
                "*.{}.{}",
                doze_names::label(&cfg.stack()),
                doze_names::SUFFIX
            )
        } else {
            "localhost".to_owned()
        };
        let dir = doze_names::ca::ca_dir(Path::new(&cfg.home));
        WireTls::from_authority(&dir, &host, t.required, CA_CHECK_EVERY)?
    } else {
        WireTls::from_files(
            &resolve(&t.cert),
            &resolve(&t.key),
            t.required,
            CA_CHECK_EVERY,
        )?
    };
    Ok(Some(tls))
}
