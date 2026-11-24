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
//! authority's own, renewed a week before it lapses. The person's files are read at each check and taken up when their
//! contents differ from what was loaded. No file time or identity is trusted: a copy keeps its time, some filesystems
//! keep coarse times, and Windows can keep a replaced file's creation time; the two files are a kilobyte or two each.
//!
//! What cannot be loaded at start stops the stack, as Go's did. A later reload that fails keeps serving the last good
//! pair, so a half-written renewal breaks no connection; it is tried again every check (a minting that failed waits
//! that long too, rather than being retried per connection), and said in the log once an hour, and again as soon as
//! the certificate still served comes within a week of lapsing or has lapsed.

use doze_names::ca::{Ca, Leaf};
pub use doze_names::certs::CA_CHECK_EVERY;
use doze_names::certs::CaSource;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

/// A certificate and its key, DER.
pub type Pair = Arc<(Vec<u8>, Vec<u8>)>;

/// How often a reload that keeps failing is said again.
const SAY_AGAIN: Duration = Duration::from_secs(3600);
/// A served certificate this close to lapsing is said at once (the authority renews its leaves this early).
const NEAR: Duration = Duration::from_secs(7 * 86_400);

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
    /// The authority's leaf `pair` was made from: an unchanged leaf (the same `Arc`) is not copied again.
    leaf: Option<Arc<Leaf>>,
    /// When the person's files were last looked at, and their bytes when `pair` was loaded from them.
    checked: Instant,
    seen: Option<Raw>,
    /// After a failed minting, nothing is tried before this.
    retry_at: Option<Instant>,
    /// A failure was said, when, and how near lapsing the served certificate was then.
    said: Option<(Instant, Lapse)>,
    /// Failures said, for the tests.
    #[cfg(test)]
    warnings: u32,
}

/// How near lapsing a served certificate is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Lapse {
    Far,
    Near,
    Lapsed,
}

/// The person's certificate and key files as read, PEM.
type Raw = (Vec<u8>, Vec<u8>);

fn read_files(cert: &Path, key: &Path) -> Result<Raw, String> {
    let read = |p: &Path| std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()));
    Ok((read(cert)?, read(key)?))
}

impl WireTls {
    /// A leaf for `host` minted from the authority under `ca_dir` (made there when there is none); its root is looked
    /// at at most every `every`, and a minting that failed is not tried again sooner. An error when no leaf can be had
    /// now.
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
        Ok(WireTls::new(from, pair, Some(first), None, required, every))
    }

    /// The person's certificate and key files, read again when they change, looked at at most every `every`. An error
    /// when they cannot be loaded now.
    pub fn from_files(
        cert: &Path,
        key: &Path,
        required: bool,
        every: Duration,
    ) -> Result<WireTls, String> {
        let pair = read_files(cert, key)
            .and_then(|raw| Ok((parse_pair(cert, key, &raw)?, raw)))
            .map_err(|e| format!("configuring TLS: loading the keypair: {e}"));
        let (pair, raw) = pair?;
        let from = Source::Files {
            cert: cert.to_path_buf(),
            key: key.to_path_buf(),
        };
        Ok(WireTls::new(
            from,
            Arc::new(pair),
            None,
            Some(raw),
            required,
            every,
        ))
    }

    fn new(
        from: Source,
        pair: Pair,
        leaf: Option<Arc<Leaf>>,
        seen: Option<Raw>,
        required: bool,
        every: Duration,
    ) -> WireTls {
        WireTls(Arc::new(Inner {
            from,
            required,
            every,
            now: Mutex::new(Now {
                pair,
                leaf,
                checked: Instant::now(),
                seen,
                retry_at: None,
                said: None,
                #[cfg(test)]
                warnings: 0,
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
        let due = |at: Instant| inner.every.is_zero() || at.elapsed() >= inner.every;
        // `None`: not looked at this time; `Some(Ok(None))`: looked at, the pair served is current.
        let looked: Option<Result<Option<Pair>, String>> = match &inner.from {
            Source::Authority { .. } if now.retry_at.is_some_and(|t| Instant::now() < t) => None,
            Source::Authority { ca, host } => {
                let leaf = ca
                    .current()
                    .ok_or_else(|| {
                        "no usable doze authority (`doze trust --rotate` makes one)".to_owned()
                    })
                    .and_then(|ca| {
                        ca.leaf(host)
                            .map_err(|e| format!("minting for {host}: {e}"))
                    });
                Some(match leaf {
                    Ok(l) if now.leaf.as_ref().is_some_and(|had| Arc::ptr_eq(had, &l)) => Ok(None),
                    Ok(l) => {
                        let p = Arc::new((l.cert_der.clone(), l.key_der.clone()));
                        now.leaf = Some(l);
                        Ok(Some(p))
                    }
                    Err(e) => {
                        now.retry_at = Some(Instant::now() + inner.every);
                        Err(e)
                    }
                })
            }
            Source::Files { .. } if !due(now.checked) => None,
            Source::Files { cert, key } => {
                now.checked = Instant::now();
                // What was seen moves only with a good load: a refused one (a renewal caught between its two writes)
                // is tried again at the next check.
                let looked = match read_files(cert, key) {
                    Ok(raw) if now.seen.as_ref() == Some(&raw) => Ok(None),
                    Ok(raw) => parse_pair(cert, key, &raw).map(|p| {
                        now.seen = Some(raw);
                        Some(Arc::new(p))
                    }),
                    Err(e) => Err(e),
                };
                Some(looked.map_err(|e| format!("reloading the keypair: {e}")))
            }
        };
        match looked {
            None => {}
            Some(Ok(p)) => {
                if let Some(p) = p {
                    now.pair = p;
                }
                now.retry_at = None;
                now.said = None;
            }
            Some(Err(e)) => now.say(&e),
        }
        now.pair.clone()
    }
}

impl Now {
    /// A failed reload, said when it is news: the first, an hour after the last, or the served certificate nearer
    /// lapsing than when last said.
    fn say(&mut self, e: &str) {
        let lapses = doze_names::ca::not_after(&self.pair.0);
        let left = lapses.and_then(|t| t.duration_since(SystemTime::now()).ok());
        let lapse = match (lapses, left) {
            (None, _) => Lapse::Far,
            (Some(_), None) => Lapse::Lapsed,
            (Some(_), Some(d)) if d <= NEAR => Lapse::Near,
            _ => Lapse::Far,
        };
        let news = self
            .said
            .is_none_or(|(at, was)| at.elapsed() >= SAY_AGAIN || lapse > was);
        if !news {
            return;
        }
        let served = match (lapse, left) {
            (Lapse::Lapsed, _) => {
                "the certificate still served has lapsed: clients that check it are refused"
                    .to_owned()
            }
            (_, Some(d)) => format!(
                "the last good certificate is still served; it lapses in {} days",
                d.as_secs() / 86_400
            ),
            _ => "the last good certificate is still served".to_owned(),
        };
        tracing::warn!("client TLS: {e}; {served}");
        self.said = Some((Instant::now(), lapse));
        #[cfg(test)]
        {
            self.warnings += 1;
        }
    }
}

/// The person's pair from the files' bytes (the first certificate, the key in the form the file has it; DER), whose key
/// must be the certificate's: a renewal caught between its two writes is not served.
fn parse_pair(cert: &Path, key: &Path, raw: &Raw) -> Result<(Vec<u8>, Vec<u8>), String> {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let c = CertificateDer::pem_slice_iter(&raw.0)
        .next()
        .ok_or_else(|| format!("{}: no certificate in it", cert.display()))?
        .map_err(|e| format!("{}: {e}", cert.display()))?
        .to_vec();
    let der =
        PrivateKeyDer::from_pem_slice(&raw.1).map_err(|e| format!("{}: {e}", key.display()))?;
    let k = der.secret_der().to_vec();
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)] // test code
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dzd-tlsu-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A self-signed pair as PEM, lapsing `life` from now.
    fn pem(life: Duration) -> (String, String) {
        let key = rcgen::KeyPair::generate().unwrap();
        let mut params = rcgen::CertificateParams::new(vec!["localhost".to_owned()]).unwrap();
        params.not_after =
            time::OffsetDateTime::now_utc() + time::Duration::try_from(life).unwrap();
        (params.self_signed(&key).unwrap().pem(), key.serialize_pem())
    }

    fn warnings(t: &WireTls) -> u32 {
        t.0.now.lock().unwrap().warnings
    }

    #[test]
    fn a_failing_reload_is_retried_and_said_hourly_and_again_as_the_served_certificate_lapses() {
        let dir = temp("say");
        let (cert, key) = (dir.join("tls.crt"), dir.join("tls.key"));
        let (c, k) = pem(Duration::from_secs(3));
        std::fs::write(&cert, &c).unwrap();
        std::fs::write(&key, &k).unwrap();
        let tls = WireTls::from_files(&cert, &key, false, Duration::ZERO).unwrap();
        let good = tls.pair();
        let loaded = tls.0.now.lock().unwrap().seen.clone();

        // Half a renewal: a new certificate beside the old key. Refused, and what was seen stays what was loaded, so
        // the next check reads the files again whatever the key's rewrite looks like.
        let (c2, k2) = pem(Duration::from_secs(86_400 * 90));
        std::fs::write(&cert, &c2).unwrap();
        for _ in 0..3 {
            assert_eq!(tls.pair(), good);
        }
        assert_eq!(tls.0.now.lock().unwrap().seen, loaded);
        assert_eq!(
            warnings(&tls),
            1,
            "said once (it lapses within a week), not per connection"
        );

        // The certificate still served lapses: said again at once, then quiet.
        std::thread::sleep(Duration::from_millis(3500));
        assert_eq!(tls.pair(), good);
        assert_eq!(warnings(&tls), 2);
        assert_eq!(tls.pair(), good);
        assert_eq!(warnings(&tls), 2);

        // The key arrives: taken up, and the next failure is news again.
        std::fs::write(&key, &k2).unwrap();
        assert_ne!(tls.pair(), good);
        assert!(tls.0.now.lock().unwrap().said.is_none());
        std::fs::write(&cert, "not a certificate").unwrap();
        tls.pair();
        assert_eq!(warnings(&tls), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
