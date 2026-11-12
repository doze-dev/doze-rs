//! The doze certificate authority: one per machine under `<home>/ca`, made the first time a domain needs https and
//! trusted once by the person. The front door mints a short-lived certificate per domain on the first handshake that
//! asks and keeps it beside the root. The key never leaves the folder; a copy lets whoever has it forge only this
//! machine's own names, the risk mkcert and OrbStack take too. Rotation makes a new one.
//!
//! The folder is shared with Go programs, so its files are Go's: `root.crt` and `root.key` (P-256, the key as SEC1
//! "EC PRIVATE KEY", which is what Go's `ParseECPrivateKey` reads), `certs/<domain>.{crt,key}`, `previous/<fp>.crt`
//! and `previous.json`.

use crate::Error;
use crate::domains::{route_domain, wildcard};
use p256::ecdsa::{DerSignature, Signature, SigningKey, VerifyingKey, signature::Verifier};
use p256::elliptic_curve::Generate;
use p256::pkcs8::LineEnding;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use x509_cert::Certificate;
use x509_cert::builder::{Builder, CertificateBuilder};
use x509_cert::der::{Decode, DecodePem, Encode, EncodePem};
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::SubjectPublicKeyInfo;
use x509_cert::time::{Time, Validity};

const ROOT_CRT: &str = "root.crt";
const ROOT_KEY: &str = "root.key";
const CERTS: &str = "certs";
const PREVIOUS_DIR: &str = "previous";
const PREVIOUS_FILE: &str = "previous.json";
const CA_LIFE: Duration = Duration::from_secs(10 * 365 * 86_400);
const LEAF_LIFE: Duration = Duration::from_secs(30 * 86_400);
const LEAF_RENEW: Duration = Duration::from_secs(7 * 86_400);

/// The authority's folder under a doze home.
pub fn ca_dir(home: &Path) -> PathBuf {
    home.join("ca")
}

fn ca_err(what: impl std::fmt::Display) -> Error {
    Error::Ca(what.to_string())
}

/// A minted leaf: its certificate (DER), its key (SEC1 DER) and when it stops being served.
#[derive(Debug)]
pub struct Leaf {
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
    pub not_after: SystemTime,
}

impl Leaf {
    fn fresh(&self) -> bool {
        SystemTime::now() + LEAF_RENEW < self.not_after
    }
}

/// The machine's authority, loaded or made.
#[derive(Debug)]
pub struct Ca {
    dir: PathBuf,
    root_der: Vec<u8>,
    root: Certificate,
    key: SigningKey,
    previous: Vec<String>,
    leaves: Mutex<HashMap<String, Arc<Leaf>>>,
}

fn random_serial() -> Result<SerialNumber, Error> {
    let mut b = [0u8; 15];
    getrandom::fill(&mut b).map_err(ca_err)?;
    if let Some(first) = b.first_mut() {
        *first &= 0x7f; // positive
    }
    SerialNumber::new(&b).map_err(ca_err)
}

fn validity(life: Duration) -> Result<Validity, Error> {
    let now = SystemTime::now();
    let from = Time::try_from(now - Duration::from_secs(3600)).map_err(ca_err)?;
    let to = Time::try_from(now + life).map_err(ca_err)?;
    Ok(Validity::new(from, to))
}

fn write_atomic(path: &Path, data: &[u8], private: bool) -> Result<(), Error> {
    // The temporary name is this process's own, so two writers never rename each other's half-written copy.
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp = path.with_extension(format!("tmp.{}.{nanos}", std::process::id()));
    if private {
        doze_os::fs::write_private(&tmp, data)?
    } else {
        std::fs::write(&tmp, data)?
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error::Io(e)
    })
}

impl Ca {
    /// The authority under `dir`, made when there is none. Two programs starting together on a fresh home make one
    /// between them (creation is under a lock; the loser loads what the winner wrote). A file that is there but
    /// cannot be read is an error, never silently replaced: the keychain may trust it.
    pub fn load_or_create(dir: &Path) -> Result<Ca, Error> {
        if let Some(ca) = Ca::probe(dir)? {
            return Ok(ca);
        }
        doze_os::fs::private_dir(dir)?;
        let _lock = doze_os::fs::lock(&dir.join(".lock"))?;
        if let Some(ca) = Ca::probe(dir)? {
            return Ok(ca);
        }
        Ca::create(dir, read_previous(dir))
    }

    /// The authority under `dir` without making one: `None` when there is none, an error when one is there and
    /// cannot be used (unreadable, or expiring within a leaf's life), which `doze trust --rotate` fixes.
    pub fn probe(dir: &Path) -> Result<Option<Ca>, Error> {
        let (crt, key) = (
            std::fs::read_to_string(dir.join(ROOT_CRT)),
            std::fs::read_to_string(dir.join(ROOT_KEY)),
        );
        let missing = |r: &std::io::Result<String>| matches!(r, Err(e) if e.kind() == std::io::ErrorKind::NotFound);
        if missing(&crt) && missing(&key) {
            return Ok(None);
        }
        let (crt, key) = (crt?, key?);
        let fix = "doze trust --rotate makes a new authority";
        let root = Certificate::from_pem(crt.as_bytes())
            .map_err(|e| ca_err(format!("{}: {e} ({fix})", dir.display())))?;
        let secret = p256::SecretKey::from_sec1_pem(&key)
            .map_err(|e| ca_err(format!("{}: the key: {e} ({fix})", dir.display())))?;
        let not_after = root.tbs_certificate().validity().not_after.to_system_time();
        if SystemTime::now() + LEAF_LIFE >= not_after {
            return Err(ca_err(format!(
                "the authority in {} expires soon: {fix}",
                dir.display()
            )));
        }
        let root_der = root.to_der().map_err(ca_err)?;
        Ok(Some(Ca {
            dir: dir.to_path_buf(),
            root_der,
            root,
            key: SigningKey::from(secret),
            previous: read_previous(dir),
            leaves: Mutex::new(HashMap::new()),
        }))
    }

    fn create(dir: &Path, previous: Vec<String>) -> Result<Ca, Error> {
        let key = SigningKey::from(p256::SecretKey::generate());
        let host = crate::ca_cert::hostname();
        let date = crate::ca_cert::today();
        let subject = Name::from_str(&format!(
            "CN=doze local CA · {} · {date},O=doze",
            host.trim_end_matches(".local")
        ))
        .map_err(ca_err)?;
        let spki = SubjectPublicKeyInfo::from_key(key.verifying_key()).map_err(ca_err)?;
        let profile = crate::ca_cert::Profile::root(subject);
        let builder = CertificateBuilder::new(profile, random_serial()?, validity(CA_LIFE)?, spki)
            .map_err(ca_err)?;
        let root = builder.build::<_, DerSignature>(&key).map_err(ca_err)?;
        let key_pem = p256::SecretKey::from(&key)
            .to_sec1_pem(LineEnding::LF)
            .map_err(ca_err)?;
        let root_pem = root.to_pem(LineEnding::LF).map_err(ca_err)?;
        std::fs::create_dir_all(dir.join(CERTS))?;
        write_atomic(&dir.join(ROOT_KEY), key_pem.as_bytes(), true)?;
        write_atomic(&dir.join(ROOT_CRT), root_pem.as_bytes(), false)?;
        // Leaves of an older authority are no good now.
        for e in std::fs::read_dir(dir.join(CERTS))?.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
        let root_der = root.to_der().map_err(ca_err)?;
        let leaves = Mutex::new(HashMap::new());
        Ok(Ca {
            dir: dir.to_path_buf(),
            root_der,
            root,
            key,
            previous,
            leaves,
        })
    }

    /// Replaces the authority with a new one; everything minted before is dropped. The old root is kept under
    /// `previous/` with its fingerprint remembered, so the next trust prompt can stop trusting it. Works on a root
    /// that cannot be read, which is when it is needed.
    pub fn rotate(dir: &Path) -> Result<Ca, Error> {
        let mut previous = read_previous(dir);
        if let Ok(pem) = std::fs::read_to_string(dir.join(ROOT_CRT))
            && let Ok(old) = Certificate::from_pem(pem.as_bytes())
            && let Ok(der) = old.to_der()
        {
            let fp = hex::encode(Sha256::digest(&der));
            std::fs::create_dir_all(dir.join(PREVIOUS_DIR))?;
            write_atomic(
                &dir.join(PREVIOUS_DIR).join(format!("{fp}.crt")),
                pem.as_bytes(),
                false,
            )?;
            if !previous.contains(&fp) {
                previous.push(fp);
            }
        }
        doze_os::fs::private_dir(dir)?;
        let _lock = doze_os::fs::lock(&dir.join(".lock"))?;
        let ca = Ca::create(dir, previous)?;
        let text = serde_json::to_string(&ca.previous).map_err(std::io::Error::other)?;
        write_atomic(&dir.join(PREVIOUS_FILE), text.as_bytes(), false)?;
        Ok(ca)
    }

    /// The root certificate (PEM): for trusting, and for `SSL_CERT_FILE` and the like.
    pub fn root_path(&self) -> PathBuf {
        self.dir.join(ROOT_CRT)
    }

    /// The root certificate's DER.
    pub fn root_der(&self) -> &[u8] {
        &self.root_der
    }

    /// SHA-256 of the root, hex: how trust stores name it.
    pub fn fingerprint(&self) -> String {
        hex::encode(Sha256::digest(&self.root_der))
    }

    /// Roots rotated out that may still be trusted on the machine, until a prompt removes them.
    pub fn previous(&self) -> &[String] {
        &self.previous
    }

    /// Where a rotated-out root is kept, for the admin trust store's removal.
    pub fn previous_path(&self, fp: &str) -> PathBuf {
        self.dir.join(PREVIOUS_DIR).join(format!("{fp}.crt"))
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

/// The fingerprints of roots rotated out, from `previous.json`; anything that is not a fingerprint is dropped (the
/// file is the person's to edit, and what it holds reaches a privileged script).
fn read_previous(dir: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(dir.join(PREVIOUS_FILE)).unwrap_or_default();
    let fps: Vec<String> = serde_json::from_str(&text).unwrap_or_default();
    fps.into_iter()
        .filter(|fp| fp.len() == 64 && fp.bytes().all(|b| b.is_ascii_hexdigit()))
        .collect()
}

/// A leaf's certificate in DER, from PEM (for the front door's own-files case and tests).
pub fn cert_der_from_pem(pem: &str) -> Option<Vec<u8>> {
    Certificate::from_pem(pem.as_bytes()).ok()?.to_der().ok()
}

/// The not-after of a DER certificate.
pub fn not_after(der: &[u8]) -> Option<SystemTime> {
    Some(
        Certificate::from_der(der)
            .ok()?
            .tbs_certificate()
            .validity()
            .not_after
            .to_system_time(),
    )
}

/// `base` with `.ext` appended (a domain already has dots: `with_extension` would replace its last label).
fn with_suffix(base: &Path, ext: &str) -> PathBuf {
    let mut s = base.as_os_str().to_os_string();
    s.push(".");
    s.push(ext);
    PathBuf::from(s)
}
