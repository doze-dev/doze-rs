//! Module resolution against a signed registry on disk: a fresh pin, a warm cache with no network, a Go (protocol 1)
//! pin resolved again, the engine gate, tampering, and the offline fallback.

use super::*;
use crate::archive::tests::targz;
use crate::registry::index::{Artifact, Release, payload};
use crate::registry::{Base, Config, signed};
use base64ct::{Base64, Encoding};
use ed25519_dalek::{Signer, SigningKey};
use std::collections::BTreeMap;
use std::time::Duration;

const TRIPLE: &str = "test-triple";

fn key() -> SigningKey {
    SigningKey::from_bytes(&[9u8; 32])
}

fn sign_hex(hex: &str) -> String {
    Base64::encode_string(&key().sign(hex.as_bytes()).to_bytes())
}

struct Reg {
    root: PathBuf,
    home: PathBuf,
}

impl Drop for Reg {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.root.parent().unwrap_or(&self.root));
    }
}

/// A registry holding `valkey` releases `(version, protocol, engines)`, each with one archive for [`TRIPLE`];
/// `extra` is written into the index as fields this doze does not know.
fn registry(tag: &str, releases: &[(&str, i64, &[&str])], extra: &str) -> Reg {
    let d = std::env::temp_dir().join(format!("doze-fetch-mod-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let dir = d.join("registry/doze/valkey");
    std::fs::create_dir_all(&dir).unwrap();
    let mut idx = Index {
        module: "valkey".into(),
        namespace: "doze".into(),
        ..Index::default()
    };
    for (v, protocol, engines) in releases {
        let a = targz(&[("bin/valkey-plugin", v.as_bytes(), true)]);
        let file = format!("valkey-{v}.tar.gz");
        std::fs::write(dir.join(&file), &a).unwrap();
        let sha = signed::sha256_hex(&a);
        let art = Artifact {
            url: file,
            sig: sign_hex(&sha),
            sha256: sha,
        };
        let rel = Release {
            protocol: *protocol,
            engines: engines.iter().map(|e| (*e).to_string()).collect(),
            artifacts: BTreeMap::from([(TRIPLE.to_string(), art)]),
            doze: String::new(),
        };
        idx.releases.insert((*v).to_string(), rel);
    }
    let head = releases.last().map(|r| r.0).unwrap_or_default();
    idx.channels = Some(BTreeMap::from([("stable".to_string(), head.to_string())]));
    idx.signature = Base64::encode_string(
        &key()
            .sign(signed::sha256_hex(payload(&idx).as_bytes()).as_bytes())
            .to_bytes(),
    );
    std::fs::write(dir.join("index.yaml"), render(&idx, extra)).unwrap();
    Reg {
        root: d.join("registry"),
        home: d.join("home"),
    }
}

fn render(idx: &Index, extra: &str) -> String {
    let mut s = format!("schema: 1\nmodule: valkey\nnamespace: doze\n{extra}releases:\n");
    for (v, r) in &idx.releases {
        s += &format!("  {v}:\n    protocol: {}\n    engines:\n", r.protocol);
        for e in &r.engines {
            s += &format!("      - \"{e}\"\n");
        }
        s += "    artifacts:\n";
        for (t, a) in &r.artifacts {
            s += &format!(
                "      {t}:\n        url: {}\n        sha256: {}\n        sig: {}\n",
                a.url, a.sha256, a.sig
            );
        }
    }
    let head = idx
        .channels
        .as_ref()
        .and_then(|c| c.get("stable"))
        .cloned()
        .unwrap_or_default();
    s + &format!(
        "channels:\n  stable: {head}\nsignature: {}\n",
        idx.signature
    )
}

fn modules(reg: &Reg) -> Modules {
    Modules {
        registry: Config {
            bases: vec![Base::Dir(reg.root.clone())],
            key: key().verifying_key(),
            budget: Duration::from_secs(3),
        },
        home: reg.home.clone(),
        triple: TRIPLE.into(),
        protocol: 2,
        doze: "0.3.0".into(),
    }
}

fn valkey() -> Source {
    Source::parse("valkey").unwrap()
}

fn majors(m: &[&str]) -> Vec<String> {
    m.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn a_fresh_resolve_pins_the_newest_fit_and_a_warm_pin_needs_no_network() {
    let reg = registry(
        "fresh",
        &[("0.2.1", 1, &["9"]), ("0.3.0", 2, &["8", "9"])],
        "",
    );
    let m = modules(&reg);
    let mut lock = Lock::default();
    let r = m.resolve(&valkey(), &majors(&["9"]), &mut lock).unwrap();
    assert_eq!(r.version, "0.3.0");
    assert_eq!(std::fs::read(&r.exe).unwrap(), b"0.3.0");
    assert!(
        r.notes
            .iter()
            .any(|n| n.starts_with("pinned doze/valkey 0.3.0"))
    );
    let pin = lock.modules.get("doze/valkey").cloned().unwrap();
    assert!(lock.dirty);
    assert_eq!((pin.version.as_str(), pin.protocol), ("0.3.0", 2));
    assert!(pin.hashes[TRIPLE].starts_with("sha256:"));
    // The registry goes away: the pin and the cache are enough.
    std::fs::remove_dir_all(&reg.root).unwrap();
    let again = m.resolve(&valkey(), &majors(&["9"]), &mut lock).unwrap();
    assert_eq!((again.version.as_str(), again.notes.len()), ("0.3.0", 0));
}

#[test]
fn a_go_protocol_1_pin_is_resolved_again_and_replaced() {
    let reg = registry("proto1", &[("0.2.1", 1, &["9"]), ("0.3.0", 2, &["9"])], "");
    let m = modules(&reg);
    let mut lock = Lock::default();
    let go_pin = Pin {
        version: "0.2.1".into(),
        protocol: 1,
        engines: majors(&["9"]),
        ..Pin::default()
    };
    lock.modules.insert("doze/valkey".into(), go_pin);
    let r = m.resolve(&valkey(), &majors(&["9"]), &mut lock).unwrap();
    assert_eq!(r.version, "0.3.0");
    assert!(
        r.notes
            .iter()
            .any(|n| n.contains("speaks module protocol 1")),
        "{:?}",
        r.notes
    );
    assert_eq!(lock.modules["doze/valkey"].protocol, 2);
}

#[test]
fn a_pin_is_held_to_its_engines_and_its_checksum() {
    let reg = registry("gate", &[("0.3.0", 2, &["9"])], "");
    let m = modules(&reg);
    let mut lock = Lock::default();
    m.resolve(&valkey(), &[], &mut lock).unwrap();
    let err = m
        .resolve(&valkey(), &majors(&["7"]), &mut lock)
        .unwrap_err();
    assert!(
        err.starts_with("valkey 7 needs a newer doze/valkey module"),
        "{err}"
    );
    if let Some(p) = lock.modules.get_mut("doze/valkey") {
        p.hashes.insert(TRIPLE.into(), "sha256:00".into());
    }
    let err = m.resolve(&valkey(), &[], &mut lock).unwrap_err();
    assert!(err.contains("but the lock pins sha256:00"), "{err}");
}

#[test]
fn a_tampered_archive_or_index_is_refused() {
    let reg = registry("tamper", &[("0.3.0", 2, &["9"])], "");
    let m = modules(&reg);
    let archive = reg.root.join("doze/valkey/valkey-0.3.0.tar.gz");
    std::fs::write(&archive, targz(&[("bin/evil", b"x", true)])).unwrap();
    let err = m.resolve(&valkey(), &[], &mut Lock::default()).unwrap_err();
    assert!(err.starts_with("checksum mismatch"), "{err}");
    let index = reg.root.join("doze/valkey/index.yaml");
    let text = std::fs::read_to_string(&index).unwrap();
    std::fs::write(&index, text.replace("protocol: 2", "protocol: 3")).unwrap();
    let err = m.resolve(&valkey(), &[], &mut Lock::default()).unwrap_err();
    assert!(err.starts_with("module index for doze/valkey"), "{err}");
}

#[test]
fn fields_this_doze_does_not_know_are_carried_and_still_verify() {
    let extra = "client:\n  program: valkey-cli\ndetect:\n  images: [valkey/valkey]\n";
    let reg = registry("extra", &[("0.3.0", 2, &["9"])], extra);
    let m = modules(&reg);
    let r = m.resolve(&valkey(), &[], &mut Lock::default()).unwrap();
    assert_eq!(r.version, "0.3.0");
}

#[test]
fn offline_with_no_pin_the_newest_cached_build_will_do() {
    let reg = registry("offline", &[("0.3.0", 2, &["9"])], "");
    let m = modules(&reg);
    m.resolve(&valkey(), &[], &mut Lock::default()).unwrap();
    std::fs::remove_dir_all(&reg.root).unwrap();
    let mut lock = Lock::default();
    let r = m.resolve(&valkey(), &[], &mut lock).unwrap();
    assert_eq!(r.version, "0.3.0");
    assert!(
        r.notes.iter().any(|n| n.contains("can't be reached")),
        "{:?}",
        r.notes
    );
    assert!(lock.modules.is_empty(), "nothing to pin from");
}

#[test]
fn nothing_published_for_this_protocol_says_so() {
    let reg = registry("noproto", &[("0.2.1", 1, &["9"])], "");
    let err = modules(&reg)
        .resolve(&valkey(), &[], &mut Lock::default())
        .unwrap_err();
    assert!(err.contains("speaks module protocol 2"), "{err}");
}

/// Go's `TestRejectUnsignedIndex`, `TestRejectRegistrySignedByAnotherKey`.
#[test]
fn an_unsigned_index_or_one_signed_by_another_key_is_refused() {
    let reg = registry("keys", &[("0.3.0", 2, &["9"])], "");
    let index = reg.root.join("doze/valkey/index.yaml");
    let signed_text = std::fs::read_to_string(&index).unwrap();
    let mut other = modules(&reg);
    other.registry.key = SigningKey::from_bytes(&[3u8; 32]).verifying_key();
    let err = other.index(&valkey()).unwrap_err();
    assert!(err.starts_with("module index for doze/valkey"), "{err}");
    let unsigned: String = signed_text
        .lines()
        .filter(|l| !l.starts_with("signature:"))
        .map(|l| format!("{l}\n"))
        .collect();
    std::fs::write(&index, unsigned).unwrap();
    let err = modules(&reg).index(&valkey()).unwrap_err();
    assert!(err.starts_with("module index for doze/valkey"), "{err}");
}

/// Go's `TestOneRegistryOneNamespace`.
#[test]
fn modules_come_from_the_doze_namespace_only() {
    let reg = registry("ns", &[("0.3.0", 2, &["9"])], "");
    let acme = Source::parse("acme/valkey").unwrap();
    let err = modules(&reg)
        .resolve(&acme, &[], &mut Lock::default())
        .unwrap_err();
    assert!(err.contains("doze has one registry"), "{err}");
}

/// Go's `TestUpgradeMovesPin`.
#[test]
fn an_upgrade_moves_the_pin_to_the_newest_fit() {
    let reg = registry("upgrade", &[("0.3.0", 2, &["9"]), ("0.3.1", 2, &["9"])], "");
    let m = modules(&reg);
    let mut lock = Lock::default();
    let old = Pin {
        version: "0.3.0".into(),
        protocol: 2,
        ..Pin::default()
    };
    lock.modules.insert("doze/valkey".into(), old);
    let (from, r) = m.upgrade(&valkey(), &[], &mut lock).unwrap();
    assert_eq!(
        (from.as_deref(), r.version.as_str()),
        (Some("0.3.0"), "0.3.1")
    );
    assert_eq!(lock.modules["doze/valkey"].version, "0.3.1");
    assert!(lock.dirty);
}

/// F1 audit blocker 2: a pin's version is a folder name, never a path: an absolute or `../` version is refused when
/// the lock is read, so nothing outside the cache can be run.
#[test]
fn a_lock_whose_version_is_a_path_is_refused() {
    for bad in [
        "/tmp/repo/vendor/x",
        "../../x",
        "..",
        ".hidden",
        "a/b",
        "a\\b",
    ] {
        let text =
            format!("modules:\n    doze/valkey:\n        version: {bad:?}\n        protocol: 2\n");
        let err = crate::lock::parse(&text).unwrap_err();
        assert!(
            err.message.contains("not a plain version"),
            "{bad}: {err:?}"
        );
    }
    assert!(crate::lock::parse("modules:\n    doze/valkey:\n        version: 0.3.0+b1\n").is_ok());
    let place_req = crate::child::Request::Ensure {
        engine: "../valkey".into(),
        full: "9.1.1".into(),
        target: "t".into(),
        expected_sha: None,
    };
    let reg = registry("plain", &[("0.3.0", 2, &["9"])], "");
    let place = crate::child::Place {
        mirror: crate::mirror::Mirror {
            root: "file:///nowhere".into(),
            home: reg.home.clone(),
            key: None,
        },
        modules: modules(&reg),
    };
    let r = crate::child::serve(&place_req, &place);
    assert!(r.error.unwrap_or_default().starts_with("refused"));
}

/// Go's `TestProtocolGates`, the other side: every release speaks a newer protocol, so doze is what needs updating.
#[test]
fn releases_for_a_newer_protocol_say_to_update_doze() {
    let reg = registry("newproto", &[("0.4.0", 3, &["9"])], "");
    let err = modules(&reg)
        .resolve(&valkey(), &[], &mut Lock::default())
        .unwrap_err();
    assert!(err.contains("Update doze"), "{err}");
}

/// F1 audit major 4: a development build never stamps `doze: dev` into the lock (Go stamps released versions only).
#[test]
fn a_development_build_does_not_stamp_the_lock() {
    let d = std::env::temp_dir().join(format!("doze-fetch-dev-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let path = d.join("doze.lock");
    let mut lock = Lock::default();
    lock.record_module(
        "doze/valkey",
        Pin {
            version: "0.3.0".into(),
            protocol: 2,
            ..Pin::default()
        },
    );
    lock.save(&path, Some("dev")).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("doze: dev"), "{text}");
    let _ = std::fs::remove_dir_all(&d);
}

/// F1 audit major 5: offline, a cached build of another protocol (the Go doze's, sharing the cache) is not taken.
#[test]
fn offline_a_build_of_another_protocol_is_not_taken() {
    let reg = registry("offproto", &[("0.3.0", 2, &["9"])], "");
    let m = modules(&reg);
    m.resolve(&valkey(), &[], &mut Lock::default()).unwrap();
    std::fs::remove_dir_all(&reg.root).unwrap();
    let marker = m.content_dir(&valkey(), "0.3.0").join(PROTOCOL_FILE);
    std::fs::write(&marker, "1\n").unwrap();
    assert!(m.resolve(&valkey(), &[], &mut Lock::default()).is_err());
}
