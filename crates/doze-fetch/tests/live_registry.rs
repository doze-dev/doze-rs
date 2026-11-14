//! The live registry (https://doze-registry.pages.dev, the Pages host of registry.doze-dev.com): every module's real
//! index verifies with the key built into doze, and its newest build for this machine downloads, matches the index's
//! checksum and signature, and unpacks to a plugin. Runs when `DOZE_LIVE_REGISTRY=1` (CI's `os` job sets it on all five
//! runners); a network test does not belong in every local `cargo test`.

use doze_fetch::modules::{Modules, plugin_exe};
use doze_fetch::registry::select::compare_versions;
use doze_fetch::registry::source::Source;
use doze_fetch::registry::{Base, Config, PAGES_BASE, signed};
use std::env::consts::{ARCH, OS};
use std::time::Duration;

/// The modules the registry publishes (its home page lists these eight).
const MODULES: [&str; 8] = [
    "aws", "ferret", "kafka", "kvrocks", "mariadb", "postgres", "temporal", "valkey",
];

fn triple() -> String {
    match OS {
        "macos" => format!("{ARCH}-apple-darwin"),
        "windows" => format!("{ARCH}-pc-windows-msvc"),
        _ => format!("{ARCH}-unknown-linux-gnu"),
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
    let mut fetched = Vec::new();
    for name in MODULES {
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
    }
    let _ = std::fs::remove_dir_all(&home);
    eprintln!("verified and unpacked: {fetched:?}");
    // Modules ship for macOS and Linux (arm64, x86-64); on Windows only the indexes are checked.
    if OS != "windows" {
        assert_eq!(fetched.len(), MODULES.len(), "{fetched:?}");
    }
}
