//! The authority: made once, read back, leaves minted and kept, rotation, a broken root never silently replaced; and,
//! since Go and Rust programs share `<home>/ca`, each reading what the other made (via tools/names-oracle; that half
//! needs Go and ../doze-names and says it skipped without them).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_names::ca::Ca;
use std::path::PathBuf;
use std::process::Command;

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("doze-names-ca-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn made_once_read_back_and_keys_as_go_writes_them() {
    let dir = scratch("made");
    let ca = Ca::load_or_create(&dir).unwrap();
    let again = Ca::load_or_create(&dir).unwrap();
    assert_eq!(
        ca.fingerprint(),
        again.fingerprint(),
        "a second open loads, never makes another"
    );
    let key = std::fs::read_to_string(dir.join("root.key")).unwrap();
    assert!(
        key.starts_with("-----BEGIN EC PRIVATE KEY-----"),
        "SEC1, what Go's ParseECPrivateKey reads: {key}"
    );
    assert!(
        Ca::probe(&scratch("none")).unwrap().is_none(),
        "probe never makes one"
    );
}

#[test]
fn a_leaf_is_minted_once_and_kept_and_a_wildcard_covers_the_name_under_it() {
    let dir = scratch("leaf");
    let ca = Ca::load_or_create(&dir).unwrap();
    let leaf = ca.leaf("dev.acme.com").unwrap();
    assert!(
        dir.join("certs/dev.acme.com.crt").exists() && dir.join("certs/dev.acme.com.key").exists()
    );
    let reopened = Ca::load_or_create(&dir).unwrap();
    assert_eq!(
        reopened.leaf("dev.acme.com").unwrap().cert_der,
        leaf.cert_der,
        "kept, not minted again"
    );
    let wild = ca.leaf("*.dev.acme.com").unwrap();
    assert!(dir.join("certs/_wildcard.dev.acme.com.crt").exists());
    let der = &wild.cert_der;
    let has = |s: &str| der.windows(s.len()).any(|w| w == s.as_bytes());
    assert!(
        has("*.dev.acme.com") && has("dev.acme.com"),
        "a wildcard's certificate carries the name under it"
    );
}

#[test]
fn rotation_makes_a_new_root_and_remembers_the_old_and_works_on_a_broken_one() {
    // Go: TestARotatedAuthorityIsSeenByTheRunningFrontDoor (the rotation half), TestRotateCAWorksOnABrokenRoot.
    let dir = scratch("rotate");
    let first = Ca::load_or_create(&dir).unwrap();
    first.leaf("dev.acme.com").unwrap();
    let second = Ca::rotate(&dir).unwrap();
    assert_ne!(first.fingerprint(), second.fingerprint());
    assert_eq!(second.previous(), [first.fingerprint()]);
    assert!(
        second.previous_path(&first.fingerprint()).exists(),
        "the old root is kept by file"
    );
    assert!(
        !dir.join("certs/dev.acme.com.crt").exists(),
        "leaves of the old root are dropped"
    );

    std::fs::write(dir.join("root.key"), "not a key").unwrap();
    assert!(
        Ca::probe(&dir).is_err(),
        "a broken key is an error to every surface"
    );
    assert!(
        Ca::load_or_create(&dir).is_err(),
        "and is never silently replaced"
    );
    let fresh = Ca::rotate(&dir).unwrap();
    assert!(fresh.previous().contains(&second.fingerprint()));
    std::fs::write(dir.join("previous.json"), r#"["../../etc/passwd","nope"]"#).unwrap();
    assert!(
        Ca::probe(&dir).unwrap().unwrap().previous().is_empty(),
        "what previous.json holds is checked"
    );
}

fn oracle() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/names-oracle");
    let go = Command::new("go")
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success());
    (go && dir.join("../../../doze-names/ca.go").exists()).then_some(dir)
}

fn go(dir: &PathBuf, args: &[&str]) -> String {
    let out = Command::new("go")
        .arg("run")
        .arg(".")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "go {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn go_and_rust_share_one_authority() {
    let Some(oracle) = oracle() else {
        eprintln!("skipped: needs Go and ../doze-names beside doze-rs");
        return;
    };
    // Rust makes the authority and a leaf; Go loads it and verifies the leaf for the name.
    let rust_made = scratch("rust-made");
    let ca = Ca::load_or_create(&rust_made).unwrap();
    ca.leaf("dev.acme.com").unwrap();
    let said = go(
        &oracle,
        &["verify", rust_made.to_str().unwrap(), "dev.acme.com"],
    );
    assert_eq!(said, format!("verified {}", ca.fingerprint()));
    // Go makes the authority and a leaf; Rust loads it, reuses Go's leaf, and mints one of its own.
    let go_made = scratch("go-made");
    let fp = go(&oracle, &["ca", go_made.to_str().unwrap(), "api.acme.com"]);
    let from_go = Ca::load_or_create(&go_made).unwrap();
    assert_eq!(from_go.fingerprint(), fp);
    let go_leaf = std::fs::read(go_made.join("certs/api.acme.com.crt")).unwrap();
    let ours = from_go.leaf("api.acme.com").unwrap();
    let pem_der = doze_names::ca::cert_der_from_pem(&String::from_utf8(go_leaf).unwrap()).unwrap();
    assert_eq!(
        ours.cert_der, pem_der,
        "Go's fresh leaf is reused, not minted again"
    );
    from_go.leaf("web.acme.com").unwrap();
    let said = go(
        &oracle,
        &["verify", go_made.to_str().unwrap(), "web.acme.com"],
    );
    assert_eq!(
        said,
        format!("verified {fp}"),
        "Go verifies a leaf Rust minted from Go's authority"
    );
}
