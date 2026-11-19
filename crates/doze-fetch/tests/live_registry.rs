//! The live registry (https://doze-registry.pages.dev, the Pages host of registry.doze-dev.com): the key it publishes
//! is the one built into doze, and every module its catalogue (`index.json`) lists has a real index that verifies with
//! that key, and a newest build for this machine that downloads, matches the index's checksum and signature, and
//! unpacks to a plugin. Runs when `DOZE_LIVE_REGISTRY=1` (CI's `os` job sets it on all five runners); a network test
//! does not belong in every local `cargo test`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_fetch::modules::{Modules, plugin_exe};
use doze_fetch::registry::select::compare_versions;
use doze_fetch::registry::source::Source;
use doze_fetch::registry::{Base, Config, PAGES_BASE, signed};
use std::env::consts::{ARCH, OS};
use std::time::Duration;

/// The modules the registry's catalogue lists in the doze namespace, and the key it names for it. The list is read,
/// not written down here: on 2026-11-18 every Go-built module was withdrawn (doze-registry #5, `withdrawn.yaml`) and
/// the registry starts again with the Rust modules, so what is published changes without a doze release.
fn published(registry: &Config) -> (Vec<String>, String) {
    let cat = registry
        .get("index.json")
        .unwrap_or_else(|e| panic!("the registry's catalogue: {}", e.reasons()));
    let cat: serde_json::Value = serde_json::from_slice(&cat.body).expect("index.json is JSON");
    let ns = &cat["namespaces"]["doze"];
    let modules = ns["modules"]
        .as_object()
        .expect("the doze namespace lists its modules")
        .keys()
        .map(|k| k.trim_start_matches("doze/").to_string())
        .collect();
    let key = ns["key"]
        .as_str()
        .expect("the doze namespace names its key");
    (modules, key.to_string())
}

fn triple() -> String {
    match OS {
        "macos" => format!("{ARCH}-apple-darwin"),
        "windows" => format!("{ARCH}-pc-windows-msvc"),
        _ => format!("{ARCH}-unknown-linux-gnu"),
    }
}

/// What `Modules::resolve` does for this doze (protocol 2) against the live registry: it takes a protocol-2 release,
/// or, while none is published (every release is still protocol 1, the Go doze's), says exactly that. Either way it
/// never takes a protocol-1 build. Protocol-2 releases come with the M track (fetch.md).
fn resolves_for_protocol_2(m: &Modules, src: &Source) {
    let mut lock = doze_fetch::lock::Lock::default();
    match m.resolve(src, &[], &mut lock) {
        Ok(r) => {
            let protocol = lock.modules.get(&src.to_string()).map(|p| p.protocol);
            assert_eq!(
                protocol,
                Some(2),
                "{src} {}: resolved to {protocol:?}",
                r.version
            );
        }
        Err(e) => assert!(
            e.contains("speaks module protocol 2")
                && e.contains("it has to be published for this doze"),
            "{src}: {e}"
        ),
    }
}

#[test]
fn every_published_module_verifies_and_unpacks() {
    if std::env::var("DOZE_LIVE_REGISTRY").as_deref() != Ok("1") {
        eprintln!("skipped: set DOZE_LIVE_REGISTRY=1 to check the live registry");
        return;
    }
    let home = std::env::temp_dir().join(format!("doze-fetch-live-{}", std::process::id()));
    let m = Modules {
        registry: Config {
            bases: vec![Base::Http(PAGES_BASE.into())],
            key: signed::builtin_key().unwrap(),
            budget: Duration::from_secs(20),
        },
        home: home.clone(),
        triple: triple(),
        protocol: 2,
        doze: "dev".into(),
    };
    let (modules, key) = published(&m.registry);
    assert_eq!(key, signed::KEY, "the catalogue's key is the one built in");
    let keys = m.registry.get("doze/keys.json").expect("doze/keys.json");
    let keys: serde_json::Value = serde_json::from_slice(&keys.body).expect("keys.json is JSON");
    assert_eq!(
        keys["key"].as_str(),
        Some(signed::KEY),
        "keys.json names the built-in key"
    );
    eprintln!("published: {modules:?}");
    let mut fetched = Vec::new();
    for name in &modules {
        let name = name.as_str();
        let src = Source::parse(name).unwrap();
        let idx = m.index(&src).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!idx.releases.is_empty(), "{name} has releases");
        let newest = idx
            .releases
            .iter()
            .filter(|(_, r)| r.artifacts.contains_key(&m.triple))
            .map(|(v, _)| v.clone())
            .max_by(|a, b| compare_versions(a, b));
        let Some(v) = newest else {
            eprintln!("{name}: no build for {}", m.triple);
            continue;
        };
        let bin = m
            .fetch(&src, &v, None)
            .unwrap_or_else(|e| panic!("{name} {v}: {e}"));
        assert!(plugin_exe(&bin, name).is_some(), "{name} {v}: no plugin");
        fetched.push(format!("{name} {v}"));
        resolves_for_protocol_2(&m, &src);
    }
    let _ = std::fs::remove_dir_all(&home);
    eprintln!("verified and unpacked: {fetched:?}");
    // Modules ship for macOS and Linux (arm64, x86-64); on Windows only the indexes are checked.
    if OS != "windows" {
        assert_eq!(fetched.len(), modules.len(), "{fetched:?}");
    }
}
