//! The lock's reader (as PR #10 had it) and its writer's rules (Go's `Record*`, `Save`).

use super::*;

/// A lock as the Go doze writes it: engines layer, modules layer, downloads.
#[test]
fn reads_what_go_writes() {
    let src = "doze: 0.2.1\nengines: {}\nmodules:\n    doze/valkey:\n        version: 0.2.1\n        protocol: 1\n        engines:\n            - \"8\"\n            - \"9\"\n        hashes:\n            aarch64-apple-darwin: sha256:4d4c\n        doze: 0.2.0\n    doze/old:\n        stable: {}\n";
    let lock = parse(src).unwrap();
    assert_eq!(lock.doze, "0.2.1");
    assert_eq!(lock.modules.len(), 1, "a versionless pin is dropped");
    assert!(lock.dirty, "and the drop is a change, as in Go");
    let pin = &lock.modules["doze/valkey"];
    assert_eq!((pin.version.as_str(), pin.protocol), ("0.2.1", 1));
    assert_eq!(pin.hashes["aarch64-apple-darwin"], "sha256:4d4c");
}

/// A merge left in the lock is the common way it breaks: said as that, at the line.
#[test]
fn a_merge_conflict_is_named_where_it_starts() {
    let src = "modules:\n<<<<<<< HEAD\n    doze/valkey:\n";
    let e = parse(src).unwrap_err();
    assert_eq!(
        (e.message.as_str(), e.at),
        ("a merge conflict was left in it", Some(9))
    );
}

#[test]
fn records_change_it_only_when_something_differs() {
    let mut l = Lock::default();
    let pin = Pin {
        version: "0.2.1".into(),
        protocol: 2,
        ..Pin::default()
    };
    l.record_module("doze/valkey", pin.clone());
    assert!(l.dirty);
    l.dirty = false;
    l.record_module("doze/valkey", pin);
    l.record_module("doze/x", Pin::default());
    assert!(
        !l.dirty,
        "the same pin, or one without a version, changes nothing"
    );
    l.record_download("https://x", "sha256:1");
    l.dirty = false;
    l.record_download("https://x", "sha256:2");
    assert_eq!(
        (l.downloads["https://x"].as_str(), l.dirty),
        ("sha256:1", false),
        "the first checksum stays"
    );
    let e = |r: &str, h: &[(&str, &str)]| EnginePin {
        resolved: r.into(),
        source: "mirror".into(),
        hashes: h.iter().map(|(a, b)| ((*a).into(), (*b).into())).collect(),
    };
    l.record_engine("postgres", "16", e("16.4.0", &[("a", "sha256:1")]));
    l.record_engine("postgres", "16", e("16.4.0", &[("b", "sha256:2")]));
    assert_eq!(
        l.engines["postgres"]["16"].hashes.len(),
        2,
        "hashes add up for one version"
    );
    l.record_engine("postgres", "16", e("16.5.0", &[("b", "sha256:3")]));
    assert_eq!(
        l.engines["postgres"]["16"].hashes.len(),
        1,
        "a new version starts over"
    );
    l.drop_module("doze/valkey");
    assert!(!l.modules.contains_key("doze/valkey"));
}

#[test]
fn save_writes_only_a_change_and_names_the_newest_doze() {
    let dir = std::env::temp_dir().join(format!("doze-fetch-lock-{}", std::process::id()));
    let path = dir.join(FILE);
    let mut l = Lock {
        doze: "0.2.0".into(),
        ..Lock::default()
    };
    l.save(&path, Some("0.3.0")).unwrap();
    assert!(!path.exists(), "nothing changed, nothing written");
    l.record_download("https://x", "sha256:1");
    l.save(&path, Some("v0.3.0")).unwrap();
    let back = Lock::load(&path).unwrap();
    assert_eq!(back.doze, "0.3.0");
    assert_eq!(back.downloads["https://x"], "sha256:1");
    let mut older = back.clone();
    older.record_download("https://y", "sha256:2");
    older.save(&path, Some("0.1.0")).unwrap();
    assert_eq!(
        Lock::load(&path).unwrap().doze,
        "0.3.0",
        "an older doze does not lower it"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
