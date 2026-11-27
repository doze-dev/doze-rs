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

/// The machine's roots and `true`, or, where it has no roots file (Windows), two stand-in public roots and `false`.
fn system_or_stand_ins() -> (String, bool) {
    let machine = system_roots_pem();
    if machine.is_empty() {
        let stand_ins = format!(
            "{}{}",
            corp_root("Public Root A"),
            corp_root("Public Root B")
        );
        (stand_ins, false)
    } else {
        (machine, true)
    }
}

/// (doze roots, certificates) in a bundle.
fn count(path: &Path) -> (usize, usize) {
    let text = std::fs::read_to_string(path).unwrap();
    let blocks = pem_blocks(&text);
    (blocks.iter().filter(|b| doze_root(b)).count(), blocks.len())
}

/// Go: TestTheBundleHoldsSystemRootsOnceAndNoOldDozeRoot. The bundle carries the machine's roots once and the current
/// doze root once: a shell that already points SSL_CERT_FILE at the bundle does not fold it in again, a rotated-out
/// doze root in an inherited file is dropped, and writers racing never leave a torn file. A machine with no roots file
/// (Windows) gets two stand-in public roots, so every assertion still runs.
#[test]
fn the_bundle_holds_system_roots_once_and_no_old_doze_root() {
    let dir = scratch("roots");
    let (system, real) = system_or_stand_ins();
    let ca = Ca::load_or_create(&dir.join("ca")).unwrap();
    let stale = Ca::load_or_create(&dir.join("stale")).unwrap();
    let corp = corp_root("Corp Root CA");
    let inherited = dir.join("corp-bundle.pem");
    let stale_pem = std::fs::read_to_string(stale.root_path()).unwrap();
    std::fs::write(&inherited, format!("{corp}{stale_pem}")).unwrap();

    let path = ca.bundle_with(&system, Some(&inherited)).unwrap();
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
    if real {
        // Where the machine has a bundle of its own (every Linux and macOS runner), it is carried.
        assert!(total > 50, "the system's roots are carried: {total}");
    } else {
        assert_eq!(
            total, 4,
            "the two stand-in roots, the corporate one and doze's"
        );
    }

    // A shell that ran eval "$(doze env)" hands the bundle back as SSL_CERT_FILE: the next write must not double it.
    ca.bundle_with(&system, Some(&path)).unwrap();
    assert_eq!(
        count(&path),
        (1, total - 1),
        "the corporate root is gone with its file, nothing else changes"
    );

    let writers: Vec<_> = (0..8)
        .map(|_| {
            let dir = dir.join("ca");
            let system = system.clone();
            std::thread::spawn(move || {
                Ca::probe(&dir)
                    .unwrap()
                    .unwrap()
                    .bundle_with(&system, None)
                    .unwrap();
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

/// A system with no roots to bundle (a bare container, a keychain that cannot be read): no bundle of the doze root
/// alone is written, and an old one of the root alone is not kept.
#[test]
fn no_system_roots_writes_no_bundle_of_the_doze_root_alone() {
    let dir = scratch("no-roots");
    let ca = Ca::load_or_create(&dir).unwrap();
    // A corporate file alone (an inherited SSL_CERT_FILE, an administrator's anchor) is not the system's roots.
    let corp = dir.join("corp.pem");
    std::fs::write(&corp, corp_root("Corp Root")).unwrap();
    assert!(ca.bundle_with("", Some(&corp)).is_err());
    assert!(!ca.bundle_path().exists());
    // Remembered: the next look does not ask the system again while nothing changed.
    assert!(dir.join(NO_ROOTS).exists());
    assert!(ca.ensure_bundle().is_err(), "the failure is kept");
    std::fs::remove_file(dir.join(NO_ROOTS)).unwrap();
    // As an older doze left it: the root alone, newer than the root.
    std::fs::copy(ca.root_path(), ca.bundle_path()).unwrap();
    if let Ok(path) = ca.ensure_bundle() {
        let text = std::fs::read_to_string(path).unwrap();
        assert!(
            pem_blocks(&text).len() > 1,
            "rewritten with the system's roots"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
