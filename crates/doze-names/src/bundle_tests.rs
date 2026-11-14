use super::*;
use crate::ca::{random_serial, validity};
use p256::ecdsa::{DerSignature, SigningKey};
use p256::elliptic_curve::Generate;
use p256::pkcs8::LineEnding;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;
use x509_cert::builder::{Builder, CertificateBuilder};
use x509_cert::der::EncodePem;
use x509_cert::name::Name;
use x509_cert::spki::SubjectPublicKeyInfo;

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("doze-names-bundle-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A self-signed authority that is not doze's.
fn corp_root(cn: &str) -> String {
    let key = SigningKey::from(p256::SecretKey::generate());
    let spki = SubjectPublicKeyInfo::from_key(key.verifying_key()).unwrap();
    let profile = crate::ca_cert::Profile::root(Name::from_str(&format!("CN={cn}")).unwrap());
    let builder = CertificateBuilder::new(
        profile,
        random_serial().unwrap(),
        validity(Duration::from_secs(86_400)).unwrap(),
        spki,
    )
    .unwrap();
    let cert = builder.build::<_, DerSignature>(&key).unwrap();
    cert.to_pem(LineEnding::LF).unwrap()
}

/// (doze roots, certificates) in a bundle.
fn count(path: &Path) -> (usize, usize) {
    let text = std::fs::read_to_string(path).unwrap();
    let blocks = pem_blocks(&text);
    (blocks.iter().filter(|b| doze_root(b)).count(), blocks.len())
}

/// Go: TestTheBundleHoldsSystemRootsOnceAndNoOldDozeRoot. The bundle carries the machine's roots once and the current
/// doze root once: a shell that already points SSL_CERT_FILE at the bundle does not fold it in again, a rotated-out
/// doze root in an inherited file is dropped, and writers racing never leave a torn file.
#[test]
fn the_bundle_holds_system_roots_once_and_no_old_doze_root() {
    let dir = scratch("roots");
    let ca = Ca::load_or_create(&dir.join("ca")).unwrap();
    let stale = Ca::load_or_create(&dir.join("stale")).unwrap();
    let corp = corp_root("Corp Root CA");
    let inherited = dir.join("corp-bundle.pem");
    let stale_pem = std::fs::read_to_string(stale.root_path()).unwrap();
    std::fs::write(&inherited, format!("{corp}{stale_pem}")).unwrap();

    let path = ca.bundle_from(Some(&inherited)).unwrap();
    let (doze, total) = count(&path);
    assert_eq!(doze, 1, "only the current doze root");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains(corp.trim()),
        "the inherited corporate root is kept"
    );
    assert!(
        !text.contains(stale_pem.trim()),
        "a rotated-out doze root in the inherited file is dropped"
    );
    assert!(text.contains(std::fs::read_to_string(ca.root_path()).unwrap().trim()));
    if total > 2 {
        // Where the machine has a bundle of its own (every Linux and macOS runner), it is carried.
        assert!(total > 50, "the system's roots are carried: {total}");
    }

    // A shell that ran eval "$(doze env)" hands the bundle back as SSL_CERT_FILE: the next write must not double it.
    ca.bundle_from(Some(&path)).unwrap();
    assert_eq!(
        count(&path),
        (1, total - 1),
        "the corporate root is gone with its file, nothing else changes"
    );

    let writers: Vec<_> = (0..8)
        .map(|_| {
            let dir = dir.join("ca");
            std::thread::spawn(move || {
                Ca::probe(&dir).unwrap().unwrap().bundle_from(None).unwrap();
            })
        })
        .collect();
    for w in writers {
        w.join().unwrap();
    }
    assert_eq!(count(&path).0, 1, "after concurrent writes");
    let leftovers = std::fs::read_dir(dir.join("ca"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
        .count();
    assert_eq!(leftovers, 0, "no temporary file is left behind");
    assert_eq!(
        ca.ensure_bundle().unwrap(),
        path,
        "a fresh bundle is not rewritten"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pem_blocks_keep_each_block_whole_and_drop_what_is_between() {
    let text = "# comment\n-----BEGIN CERTIFICATE-----\nAAA\n-----END CERTIFICATE-----\nnoise\n\
                -----BEGIN TRUSTED CERTIFICATE-----\nBBB\n-----END TRUSTED CERTIFICATE-----\n-----BEGIN torn";
    assert_eq!(
        pem_blocks(text),
        [
            "-----BEGIN CERTIFICATE-----\nAAA\n-----END CERTIFICATE-----",
            "-----BEGIN TRUSTED CERTIFICATE-----\nBBB\n-----END TRUSTED CERTIFICATE-----"
        ]
    );
}
