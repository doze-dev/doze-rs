//! The certificate handed to a module terminating its clients' TLS (`wire.accept`) is the one current now, not the one
//! the stack loaded with: the authority's leaf renewed before it lapses, a rotated authority followed, the person's
//! renewed files picked up, and a broken replacement never served in place of the last good pair.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test code

use doze_daemon::tls::WireTls;
use doze_names::ca::Ca;
use p256::pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// A throwaway doze home's authority folder.
fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("dzd-tls-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A TLS handshake from a client trusting only `root` for `host` against a server holding `pair`.
async fn handshake(pair: &(Vec<u8>, Vec<u8>), root: &[u8], host: &str) -> Result<(), String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let key = PrivateKeyDer::try_from(pair.1.clone()).unwrap();
    let server = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![CertificateDer::from(pair.0.clone())], key)
        .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(CertificateDer::from(root.to_vec())).unwrap();
    let client = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let (c, s) = tokio::io::duplex(64 * 1024);
    let accept = tokio_rustls::TlsAcceptor::from(Arc::new(server)).accept(s);
    let name = ServerName::try_from(host.to_owned()).unwrap();
    let connect = tokio_rustls::TlsConnector::from(Arc::new(client)).connect(name, c);
    let (a, c) = tokio::join!(accept, connect);
    c.map_err(|e| e.to_string())?;
    a.map_err(|e| e.to_string())?;
    Ok(())
}

fn root_der(dir: &Path) -> Vec<u8> {
    Ca::probe(dir).unwrap().unwrap().root_der().to_vec()
}

/// Puts a leaf for `host` beside the authority's root, signed by it, that stops being fresh (a week before it lapses)
/// `left` from now: the leaf a daemon would be holding after three weeks of running. Its certificate, DER.
fn plant_aging_leaf(dir: &Path, host: &str, left: Duration) -> Vec<u8> {
    let root_pem = std::fs::read_to_string(dir.join("root.crt")).unwrap();
    let root_key =
        p256::SecretKey::from_sec1_pem(&std::fs::read_to_string(dir.join("root.key")).unwrap())
            .unwrap();
    let signer = rcgen::KeyPair::try_from(root_key.to_pkcs8_der().unwrap().as_bytes()).unwrap();
    let issuer = rcgen::Issuer::from_ca_cert_pem(&root_pem, signer).unwrap();
    let key = rcgen::KeyPair::generate().unwrap();
    let mut params = rcgen::CertificateParams::new(vec![host.to_owned()]).unwrap();
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::hours(1);
    params.not_after = now + time::Duration::days(7) + time::Duration::try_from(left).unwrap();
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    let cert = params.signed_by(&key, &issuer).unwrap();
    let sec1 = p256::SecretKey::from_pkcs8_der(&key.serialize_der())
        .unwrap()
        .to_sec1_pem(LineEnding::LF)
        .unwrap();
    let base = dir.join("certs");
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join(format!("{host}.crt")), cert.pem()).unwrap();
    std::fs::write(base.join(format!("{host}.key")), sec1.as_bytes()).unwrap();
    cert.der().to_vec()
}

#[tokio::test]
async fn a_leaf_that_is_no_longer_fresh_is_renewed_for_the_next_connection() {
    let dir = temp("renew");
    Ca::load_or_create(&dir).unwrap();
    let root = root_der(&dir);
    let left = Duration::from_secs(3);
    let aging = plant_aging_leaf(&dir, "localhost", left);
    let tls = WireTls::from_authority(&dir, "localhost", false, Duration::ZERO).unwrap();
    let first = tls.pair();
    assert_eq!(first.0, aging, "the stack loads with the leaf it finds");
    handshake(&first, &root, "localhost").await.unwrap();

    // Weeks pass, as far as the leaf is concerned: it is within a week of lapsing now.
    tokio::time::sleep(left + Duration::from_millis(500)).await;
    let next = tls.pair();
    assert_ne!(next.0, aging, "the next connection gets a renewed leaf");
    let lapses = doze_names::ca::not_after(&next.0).unwrap();
    assert!(
        lapses > SystemTime::now() + Duration::from_secs(29 * 86_400),
        "a whole leaf's life ahead"
    );
    handshake(&next, &root, "localhost").await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_rotated_authority_is_followed() {
    let dir = temp("rotate");
    Ca::load_or_create(&dir).unwrap();
    let old = root_der(&dir);
    let tls = WireTls::from_authority(&dir, "*.shop.doze", true, Duration::ZERO).unwrap();
    assert!(tls.required());
    let before = tls.pair();
    handshake(&before, &old, "db.shop.doze").await.unwrap();

    Ca::rotate(&dir).unwrap(); // `doze trust --rotate`, from another process
    let new = root_der(&dir);
    assert_ne!(new, old);
    let after = tls.pair();
    handshake(&after, &new, "db.shop.doze").await.unwrap();
    let e = handshake(&after, &old, "db.shop.doze").await.unwrap_err();
    assert!(
        e.contains("BadSignature") || e.contains("UnknownIssuer"),
        "{e}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn the_root_is_looked_at_no_more_often_than_asked() {
    let dir = temp("every");
    Ca::load_or_create(&dir).unwrap();
    let tls = WireTls::from_authority(&dir, "localhost", false, Duration::from_secs(3600)).unwrap();
    let before = tls.pair();
    Ca::rotate(&dir).unwrap();
    assert_eq!(
        tls.pair(),
        before,
        "within the check interval the last root is used"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A self-signed pair as PEM files would hold them, and as DER.
struct Own {
    cert_pem: String,
    key_pem: String,
    der: (Vec<u8>, Vec<u8>),
}

fn own(names: &[&str]) -> Own {
    let key = rcgen::KeyPair::generate().unwrap();
    let cert =
        rcgen::CertificateParams::new(names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>())
            .unwrap()
            .self_signed(&key)
            .unwrap();
    Own {
        cert_pem: cert.pem(),
        key_pem: key.serialize_pem(),
        der: (cert.der().to_vec(), key.serialize_der()),
    }
}

#[tokio::test]
async fn the_persons_files_are_read_again_when_they_change_and_a_broken_one_is_never_served() {
    let dir = temp("own");
    let (cert, key) = (dir.join("tls.crt"), dir.join("tls.key"));
    let write = |c: &str, k: &str| {
        std::fs::write(&cert, c).unwrap();
        std::fs::write(&key, k).unwrap();
    };
    let a = own(&["localhost"]);
    write(&a.cert_pem, &a.key_pem);
    let tls = WireTls::from_files(&cert, &key, false, Duration::ZERO).unwrap();
    assert_eq!(*tls.pair(), a.der);

    // Renewed on disk: the next connection gets the new pair, no restart.
    let b = own(&["localhost", "db.example.test"]);
    write(&b.cert_pem, &b.key_pem);
    assert_eq!(*tls.pair(), b.der);
    handshake(&tls.pair(), &b.der.0, "db.example.test")
        .await
        .unwrap();

    // A broken certificate, then a certificate whose key is not the one beside it: the last good pair is kept.
    std::fs::write(&cert, "not a certificate").unwrap();
    assert_eq!(*tls.pair(), b.der);
    write(&a.cert_pem, &b.key_pem);
    assert_eq!(*tls.pair(), b.der);
    std::fs::remove_file(&key).unwrap();
    assert_eq!(*tls.pair(), b.der);

    // A good pair again is taken up.
    let c = own(&["localhost", "c.example.test", "more.example.test"]);
    write(&c.cert_pem, &c.key_pem);
    assert_eq!(*tls.pair(), c.der);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn files_that_cannot_be_loaded_at_start_stop_the_stack() {
    let dir = temp("missing");
    let e = WireTls::from_files(
        &dir.join("none.crt"),
        &dir.join("none.key"),
        true,
        Duration::ZERO,
    )
    .unwrap_err();
    assert!(
        e.starts_with("configuring TLS: loading the keypair:"),
        "{e}"
    );
    let a = own(&["localhost"]);
    let b = own(&["localhost"]);
    std::fs::write(dir.join("a.crt"), &a.cert_pem).unwrap();
    std::fs::write(dir.join("b.key"), &b.key_pem).unwrap();
    let e = WireTls::from_files(&dir.join("a.crt"), &dir.join("b.key"), true, Duration::ZERO)
        .unwrap_err();
    assert!(e.contains("not the certificate's"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A pair whose certificate PEM is exactly `len` bytes (an ECDSA signature's length varies by a byte or two).
fn own_of_len(len: usize) -> Own {
    (0..200)
        .map(|_| own(&["localhost"]))
        .find(|o| o.cert_pem.len() == len)
        .expect("a certificate of that length")
}

#[test]
fn a_replacement_keeping_size_and_time_is_picked_up() {
    let dir = temp("same");
    let (cert, key) = (dir.join("tls.crt"), dir.join("tls.key"));
    let a = own(&["localhost"]);
    std::fs::write(&cert, &a.cert_pem).unwrap();
    std::fs::write(&key, &a.key_pem).unwrap();
    let tls = WireTls::from_files(&cert, &key, false, Duration::ZERO).unwrap();
    assert_eq!(*tls.pair(), a.der);
    let times = |p: &Path| std::fs::metadata(p).unwrap().modified().unwrap();
    let (tc, tk) = (times(&cert), times(&key));

    // Moved into place with the old size and time (rsync -t, tar).
    let b = own_of_len(a.cert_pem.len());
    assert_eq!(b.key_pem.len(), a.key_pem.len());
    for (path, pem, t) in [(&cert, &b.cert_pem, tc), (&key, &b.key_pem, tk)] {
        let tmp = dir.join("incoming");
        std::fs::write(&tmp, pem).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&tmp)
            .unwrap()
            .set_modified(t)
            .unwrap();
        std::fs::rename(&tmp, path).unwrap();
    }
    assert_eq!(times(&cert), tc);
    assert_eq!(*tls.pair(), b.der, "a file moved into place is read");

    // Copied over in place with the time kept (cp -p): only the contents differ.
    {
        let c = own_of_len(a.cert_pem.len());
        for (path, pem, t) in [(&cert, &c.cert_pem, tc), (&key, &c.key_pem, tk)] {
            std::fs::write(path, pem).unwrap();
            std::fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(t)
                .unwrap();
        }
        assert_eq!(times(&cert), tc);
        assert_eq!(
            *tls.pair(),
            c.der,
            "a copy in place keeping the time is read"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A minting that fails (the certificates folder read-only) is not retried per connection: the last good leaf is
/// served until the check interval has passed, then the next try succeeds.
#[cfg(unix)]
#[tokio::test]
async fn a_failed_minting_waits_for_the_check_interval() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp("backoff");
    Ca::load_or_create(&dir).unwrap();
    let root = root_der(&dir);
    let left = Duration::from_secs(2);
    let aging = plant_aging_leaf(&dir, "localhost", left);
    let every = Duration::from_secs(2);
    let tls = WireTls::from_authority(&dir, "localhost", false, every).unwrap();
    assert_eq!(tls.pair().0, aging);
    tokio::time::sleep(left + Duration::from_millis(300)).await;

    let certs = dir.join("certs");
    std::fs::set_permissions(&certs, std::fs::Permissions::from_mode(0o555)).unwrap();
    if std::fs::write(certs.join("probe"), b"").is_ok() {
        eprintln!("skipped: a read-only folder is writable here (root)");
        std::fs::set_permissions(&certs, std::fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    assert_eq!(
        tls.pair().0,
        aging,
        "minting failed: the last good leaf is served"
    );
    std::fs::set_permissions(&certs, std::fs::Permissions::from_mode(0o755)).unwrap();
    // Minting would succeed now, but is not tried again before the interval.
    for _ in 0..5 {
        assert_eq!(tls.pair().0, aging);
    }
    tokio::time::sleep(every + Duration::from_millis(200)).await;
    let renewed = tls.pair();
    assert_ne!(renewed.0, aging);
    handshake(&renewed, &root, "localhost").await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
