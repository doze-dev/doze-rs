//! Leaf certificates: minted per domain from the machine's authority on the first handshake that asks, kept under
//! `certs/` (Go's names and formats), reused while fresh and signed by this root, renewed a week before they lapse.
//!
//! A name under a registered one (`abc.execute-api.us-east-1.files.shop.doze`, which doze-aws mints at run time) gets
//! a leaf for exactly that name, kept under `certs/under/`. Nothing bounds how many such names a program makes, so
//! these are bounded: at most [`UNDER_CAP`] in memory (the least recently served goes) and on disk (the oldest file
//! goes).

use crate::Error;
use crate::ca::{CERTS, Ca, LEAF_LIFE, Leaf, UNDER, ca_err, random_serial, validity, write_atomic};
use crate::domains::{route_domain, wildcard};
use p256::ecdsa::{DerSignature, Signature, SigningKey, VerifyingKey, signature::Verifier};
use p256::elliptic_curve::Generate;
use p256::pkcs8::LineEnding;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::SystemTime;
use x509_cert::Certificate;
use x509_cert::builder::{Builder, CertificateBuilder};
use x509_cert::der::{DecodePem, Encode, EncodePem};
use x509_cert::name::Name;
use x509_cert::spki::SubjectPublicKeyInfo;

/// How many leaves for names under a registered one are kept, in memory and on disk. A person works with a handful of
/// APIs and function URLs at a time, and a leaf costs about 1.5 KB and a millisecond to mint, so 256 keeps every
/// working set warm while a program that mints a fresh API id per test run leaves at most ~400 KB behind.
pub const UNDER_CAP: usize = 256;

/// The leaves for names under a registered one, by exact name, each with when it was last served.
#[derive(Debug, Default)]
pub(crate) struct Under {
    leaves: HashMap<String, (Arc<Leaf>, u64)>,
    tick: u64,
}

impl Under {
    fn get(&mut self, name: &str) -> Option<Arc<Leaf>> {
        self.tick += 1;
        let tick = self.tick;
        let (leaf, used) = self.leaves.get_mut(name).filter(|(l, _)| l.fresh())?;
        *used = tick;
        Some(leaf.clone())
    }

    fn put(&mut self, name: &str, leaf: Arc<Leaf>) {
        self.tick += 1;
        if !self.leaves.contains_key(name) && self.leaves.len() >= UNDER_CAP {
            let oldest = self
                .leaves
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(n, _)| n.clone());
            if let Some(n) = oldest {
                self.leaves.remove(&n);
            }
        }
        self.leaves.insert(name.to_string(), (leaf, self.tick));
    }
}

/// Whether `name` is a host name safe to be a file name: DNS letters, digits, hyphens and dots, no empty label.
fn plain_host(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && l.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        })
}

impl Ca {
    /// The certificate for exactly `name`, a name under a registered one: from memory, from `certs/under/`, or minted
    /// now; the caller has checked that a registered name owns it. Bounded by [`UNDER_CAP`].
    pub fn leaf_under(&self, name: &str) -> Result<Arc<Leaf>, Error> {
        if !plain_host(name) {
            return Err(ca_err(format!("{name:?} is not a host name")));
        }
        let mut under = self
            .under
            .lock()
            .map_err(|_| ca_err("the leaf cache lock is poisoned"))?;
        if let Some(l) = under.get(name) {
            return Ok(l);
        }
        let dir = self.dir.join(CERTS).join(UNDER);
        let base = dir.join(name);
        let leaf = match self.load_leaf(&base) {
            Some(l) => l,
            None => {
                std::fs::create_dir_all(&dir)?;
                let l = Arc::new(self.mint(name, &base)?);
                prune(&dir, UNDER_CAP);
                l
            }
        };
        under.put(name, leaf.clone());
        Ok(leaf)
    }

    /// The certificate for a registered domain: from memory, from `certs/`, or minted now. A wildcard's carries the
    /// wildcard and the name under it.
    pub fn leaf(&self, domain: &str) -> Result<Arc<Leaf>, Error> {
        let mut leaves = self
            .leaves
            .lock()
            .map_err(|_| ca_err("the leaf cache lock is poisoned"))?;
        if let Some(l) = leaves.get(domain).filter(|l| l.fresh()) {
            return Ok(l.clone());
        }
        let base = self.dir.join(CERTS).join(domain.replace('*', "_wildcard"));
        let leaf = match self.load_leaf(&base) {
            Some(l) => l,
            None => Arc::new(self.mint(domain, &base)?),
        };
        leaves.insert(domain.to_string(), leaf.clone());
        Ok(leaf)
    }

    /// A leaf from disk, if it is fresh and this root signed it.
    fn load_leaf(&self, base: &Path) -> Option<Arc<Leaf>> {
        let crt = std::fs::read_to_string(with_suffix(base, "crt")).ok()?;
        let key = std::fs::read_to_string(with_suffix(base, "key")).ok()?;
        let cert = Certificate::from_pem(crt.as_bytes()).ok()?;
        let leaf = Leaf {
            cert_der: cert.to_der().ok()?,
            key_der: p256::SecretKey::from_sec1_pem(&key)
                .ok()?
                .to_sec1_der()
                .ok()?
                .to_vec(),
            not_after: cert.tbs_certificate().validity().not_after.to_system_time(),
        };
        (leaf.fresh() && self.signed(&cert)).then(|| Arc::new(leaf))
    }

    /// Whether this root's key signed `cert`.
    fn signed(&self, cert: &Certificate) -> bool {
        let Ok(tbs) = cert.tbs_certificate().to_der() else {
            return false;
        };
        let Some(sig) = cert
            .signature()
            .as_bytes()
            .and_then(|b| Signature::from_der(b).ok())
        else {
            return false;
        };
        VerifyingKey::from(&self.key).verify(&tbs, &sig).is_ok()
    }

    fn mint(&self, domain: &str, base: &Path) -> Result<Leaf, Error> {
        let key = SigningKey::from(p256::SecretKey::generate());
        let mut names = vec![domain.to_string()];
        if wildcard(domain) {
            names.push(route_domain(domain));
        }
        let subject =
            Name::from_str(&format!("CN={},O=doze", route_domain(domain))).map_err(ca_err)?;
        let spki = SubjectPublicKeyInfo::from_key(key.verifying_key()).map_err(ca_err)?;
        let issuer_spki =
            SubjectPublicKeyInfo::from_key(self.key.verifying_key()).map_err(ca_err)?;
        let issuer = self.root.tbs_certificate().subject().clone();
        let profile = crate::ca_cert::Profile::leaf(subject, issuer, names, issuer_spki);
        let builder =
            CertificateBuilder::new(profile, random_serial()?, validity(LEAF_LIFE)?, spki)
                .map_err(ca_err)?;
        let cert = builder
            .build::<_, DerSignature>(&self.key)
            .map_err(ca_err)?;
        let secret = p256::SecretKey::from(&key);
        std::fs::create_dir_all(self.dir.join(CERTS))?;
        let key_pem = secret.to_sec1_pem(LineEnding::LF).map_err(ca_err)?;
        write_atomic(&with_suffix(base, "key"), key_pem.as_bytes(), true)?;
        write_atomic(
            &with_suffix(base, "crt"),
            cert.to_pem(LineEnding::LF).map_err(ca_err)?.as_bytes(),
            false,
        )?;
        Ok(Leaf {
            cert_der: cert.to_der().map_err(ca_err)?,
            key_der: secret.to_sec1_der().map_err(ca_err)?.to_vec(),
            not_after: SystemTime::now() + LEAF_LIFE,
        })
    }
}

/// Keeps at most `cap` leaves in `dir`: the oldest certificates go, with their keys. Another program sharing the
/// folder may prune at the same time; a file already gone is fine.
fn prune(dir: &Path, cap: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut crts: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "crt"))
        .map(|p| {
            let at = std::fs::metadata(&p)
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (at, p)
        })
        .collect();
    if crts.len() <= cap {
        return;
    }
    crts.sort();
    let extra = crts.len() - cap;
    for (_, crt) in crts.into_iter().take(extra) {
        let _ = std::fs::remove_file(crt.with_extension("key"));
        let _ = std::fs::remove_file(&crt);
    }
}

/// `base` with `.ext` appended (a domain already has dots: `with_extension` would replace its last label).
fn with_suffix(base: &Path, ext: &str) -> PathBuf {
    let mut s = base.as_os_str().to_os_string();
    s.push(".");
    s.push(ext);
    PathBuf::from(s)
}
