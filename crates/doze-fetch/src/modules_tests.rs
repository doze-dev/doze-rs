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
