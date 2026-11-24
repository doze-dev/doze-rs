//! Certificates for names under a registered one (`abc.execute-api.us-east-1.files.shop.doze` under
//! `files.shop.doze`): a leaf for exactly the name asked, the owner's own leaf for the owner, nothing for a name nobody
//! registered or nobody fronts, at most `UNDER_CAP` of them kept in memory and on disk, and none surviving a rotation.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_names::ca::{Ca, UNDER_CAP};
use doze_names::certs::{CaSource, Certs};
use doze_names::{Name, Registry};
use std::path::{Path, PathBuf};
use std::time::Duration;
use x509_cert::Certificate;
use x509_cert::der::Decode;
use x509_cert::ext::pkix::SubjectAltName;
use x509_cert::ext::pkix::name::GeneralName;

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("doze-names-under-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The DNS names a certificate is for.
fn sans(der: &[u8]) -> Vec<String> {
    let cert = Certificate::from_der(der).unwrap();
    let (_, san) = cert
        .tbs_certificate()
        .get_extension::<SubjectAltName>()
        .unwrap()
        .unwrap();
    san.0
        .iter()
        .filter_map(|g| match g {
            GeneralName::DnsName(n) => Some(n.to_string()),
            _ => None,
        })
        .collect()
}

/// The names served for `name` by the front door's resolver, or `None` when it serves nothing.
fn served(certs: &Certs, name: &str) -> Option<Vec<String>> {
    certs.for_name(name).map(|k| sans(k.cert[0].as_ref()))
}

fn certs(home: &Path, reg: &Registry) -> Certs {
    let dir = home.join("ca");
    Ca::load_or_create(&dir).unwrap();
    Certs::new(reg.clone(), CaSource::new(&dir, Duration::ZERO))
}

#[test]
fn a_name_under_an_instance_gets_a_leaf_for_itself_and_the_owner_keeps_its_own() {
    let home = scratch("exact");
    let reg = Registry::open(&home, "doze-aws");
    reg.claim(&Name::qualified("files", "shop"))
        .unwrap()
        .route("127.0.0.1:1")
        .unwrap();
    let certs = certs(&home, &reg);
    let under = "abc.execute-api.us-east-1.files.shop.doze";
    assert_eq!(served(&certs, under), Some(vec![under.to_string()]));
    assert_eq!(
        served(&certs, "ABC.execute-api.us-east-1.files.shop.doze."),
        Some(vec![under.to_string()]),
        "the name as a resolver compares it"
    );
    assert_eq!(
        served(&certs, "files.shop.doze"),
        Some(vec!["files.shop.doze".to_string()]),
        "the owner is served its own leaf"
    );
    assert!(home.join("ca/certs/files.shop.doze.crt").exists());
    assert!(
        home.join(format!("ca/certs/under/{under}.crt")).exists()
            && home.join(format!("ca/certs/under/{under}.key")).exists(),
        "kept on disk beside the others, apart"
    );
}

#[test]
fn a_name_nobody_registered_or_fronts_gets_nothing() {
    let home = scratch("none");
    let reg = Registry::open(&home, "doze-aws");
    reg.claim(&Name::qualified("files", "shop"))
        .unwrap()
        .route("127.0.0.1:1")
        .unwrap();
    // Registered for DNS alone, no route: the front door does not answer it, over http or https.
    let _dns_only = reg.claim(&Name::qualified("db", "shop")).unwrap();
    let certs = certs(&home, &reg);
    for name in [
        "abc.files.shop.dozer.doze",
        "shop.doze",
        "doze",
        "nothing.doze",
        "x.db.shop.doze",
        "db.shop.doze",
        "files.shop.doze.evil.com",
    ] {
        assert_eq!(served(&certs, name), None, "{name}");
    }
    assert!(
        !home.join("ca/certs/under").exists(),
        "nothing minted for a stranger"
    );
}

#[test]
fn under_a_wildcard_domain_one_level_takes_the_wildcard_and_deeper_gets_its_own() {
    let home = scratch("wild");
    let reg = Registry::open(&home, "doze");
    let web = reg.claim(&Name::qualified("web", "shop")).unwrap();
    let dom = web.twin(&Name::domain("*.dev.acme.com")).unwrap();
    dom.route("127.0.0.1:1").unwrap();
    let certs = certs(&home, &reg);
    let wild = Some(vec![
        "*.dev.acme.com".to_string(),
        "dev.acme.com".to_string(),
    ]);
    assert_eq!(served(&certs, "api.dev.acme.com"), wild);
    assert_eq!(served(&certs, "dev.acme.com"), wild);
    assert_eq!(
        served(&certs, "a.b.dev.acme.com"),
        Some(vec!["a.b.dev.acme.com".to_string()]),
        "a wildcard certificate covers one label; deeper names get their own"
    );
}

#[test]
fn leaves_under_a_name_are_bounded_in_memory_and_on_disk() {
    let home = scratch("cap");
    let dir = home.join("ca");
    let ca = Ca::load_or_create(&dir).unwrap();
    let name = |i: usize| format!("api{i}.execute-api.us-east-1.files.shop.doze");
    let first = ca.leaf_under(&name(0)).unwrap();
    // The oldest file by some margin, whatever the file system's clock resolution.
    std::thread::sleep(Duration::from_millis(20));
    let second = ca.leaf_under(&name(1)).unwrap();
    for i in 2..UNDER_CAP {
        ca.leaf_under(&name(i)).unwrap();
    }
    // Served again just now: name 1 is now the least recently served.
    assert_eq!(ca.leaf_under(&name(0)).unwrap().cert_der, first.cert_der);
    ca.leaf_under(&name(UNDER_CAP)).unwrap();
    let under = dir.join("certs/under");
    let on_disk = |ext: &str| {
        std::fs::read_dir(&under)
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == ext))
            .count()
    };
    assert_eq!(on_disk("crt"), UNDER_CAP, "the oldest file goes");
    assert_eq!(on_disk("key"), UNDER_CAP, "with its key");
    assert!(
        !under.join(format!("{}.crt", name(0))).exists(),
        "the oldest on disk is the first minted"
    );
    assert_eq!(
        ca.leaf_under(&name(0)).unwrap().cert_der,
        first.cert_der,
        "recently served, it stays in memory though its file went"
    );
    std::fs::remove_file(under.join(format!("{}.crt", name(1)))).unwrap();
    assert_ne!(
        ca.leaf_under(&name(1)).unwrap().cert_der,
        second.cert_der,
        "the least recently served went from memory: with its file gone too, it is minted again"
    );
    assert!(
        ca.leaf_under("../../root").is_err() && ca.leaf_under("a/b.doze").is_err(),
        "never a path"
    );
}

#[test]
fn a_rotation_drops_the_leaves_under_names() {
    let home = scratch("rotate");
    let dir = home.join("ca");
    let ca = Ca::load_or_create(&dir).unwrap();
    let under = "abc.execute-api.us-east-1.files.shop.doze";
    let old = ca.leaf_under(under).unwrap();
    let rotated = Ca::rotate(&dir).unwrap();
    assert!(
        !dir.join("certs/under").exists(),
        "leaves of the old root go with it"
    );
    assert_ne!(rotated.leaf_under(under).unwrap().cert_der, old.cert_der);
}
