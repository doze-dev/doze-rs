//! Why a handshake gets no certificate, said (the client sees only an alert, so the front door's log must say it), and
//! the first handshake that needs the authority making it: a routed name then gets a leaf a client can name the issuer
//! of, rather than a bare alert.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use doze_names::certs::{CaSource, Certs, Refused};
use doze_names::{Name, Registry};
use std::path::PathBuf;
use std::time::Duration;

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("doze-names-refused-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_routed_name_gets_a_leaf_with_the_authority_made_on_first_need() {
    let home = scratch("ensure");
    let reg = Registry::open(&home, "doze");
    let lease = reg.claim(&Name::qualified("web", "shop")).unwrap();
    lease.route("127.0.0.1:1").unwrap();
    let dir = home.join("ca");
    let certs = Certs::new(reg.clone(), CaSource::new(&dir, Duration::ZERO));

    assert_eq!(
        certs.choose("nobody.shop.doze").err(),
        Some(Refused::NotFronted)
    );
    assert!(!dir.exists(), "a name nobody fronts makes nothing");
    assert!(certs.choose("web.shop.doze").is_ok());
    assert!(dir.join("root.crt").is_file() && dir.join("root.key").is_file());
    let root = std::fs::read(dir.join("root.crt")).unwrap();
    certs.ca().ensure().unwrap();
    assert_eq!(
        std::fs::read(dir.join("root.crt")).unwrap(),
        root,
        "the one there is is kept"
    );
    drop(lease);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn a_broken_authority_is_never_replaced_by_the_holder() {
    let home = scratch("broken");
    let dir = home.join("ca");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("root.crt"), "not a certificate").unwrap();
    std::fs::write(dir.join("root.key"), "not a key").unwrap();
    let reg = Registry::open(&home, "doze");
    let lease = reg.claim(&Name::qualified("web", "shop")).unwrap();
    lease.route("127.0.0.1:1").unwrap();
    let certs = Certs::new(reg.clone(), CaSource::new(&dir, Duration::ZERO));
    assert!(certs.ca().ensure().is_err());
    let why = certs.choose("web.shop.doze").err().unwrap();
    assert!(
        matches!(&why, Refused::NoAuthority(_)) && why.to_string().contains("doze trust --rotate"),
        "the refusal names the fix: {why}"
    );
    drop(lease);
    assert_eq!(
        std::fs::read_to_string(dir.join("root.crt")).unwrap(),
        "not a certificate",
        "the keychain may trust what is there: doze trust --rotate replaces it, nothing else"
    );
    let _ = std::fs::remove_dir_all(&home);
}
