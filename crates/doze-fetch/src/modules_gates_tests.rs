//! More of Go's module gates, each against a signed registry on disk (audit F1, minors 3, 11, 12, 13).

use super::tests::{majors, modules, registry, valkey};
use super::*;

/// Go's `TestEngineSupportGates`, fresh path: nothing runs the majors asked for, in Go's words (`EngineSupportError`).
#[test]
fn a_fresh_resolve_names_what_the_newest_release_supports() {
    let reg = registry(
        "supports",
        &[("0.2.0", 2, &["8"]), ("0.3.0", 2, &["9"])],
        "",
    );
    let err = modules(&reg)
        .resolve(&valkey(), &majors(&["7"]), &mut Lock::default())
        .unwrap_err();
    assert_eq!(
        err,
        "no release of valkey supports valkey 7; latest (0.3.0) supports 9"
    );
}

/// Go's `TestUpgradeMovesPin`: a pin holds while a newer release appears; an upgrade moves it; a second changes
/// nothing.
#[test]
fn a_pin_holds_until_an_upgrade_and_a_second_upgrade_changes_nothing() {
    let first = registry("pinhold", &[("0.3.0", 2, &["9"])], "");
    let mut lock = Lock::default();
    modules(&first).resolve(&valkey(), &[], &mut lock).unwrap();
    let reg = registry("pinhold", &[("0.3.0", 2, &["9"]), ("0.3.1", 2, &["9"])], "");
    let m = modules(&reg);
    let held = m.resolve(&valkey(), &[], &mut lock).unwrap();
    assert_eq!(held.version, "0.3.0", "the pin wins over a newer release");
    let (from, r) = m.upgrade(&valkey(), &[], &mut lock).unwrap();
    assert_eq!(
        (from.as_deref(), r.version.as_str()),
        (Some("0.3.0"), "0.3.1")
    );
    lock.dirty = false;
    let (from, r) = m.upgrade(&valkey(), &[], &mut lock).unwrap();
    assert_eq!(
        (from.as_deref(), r.version.as_str()),
        (Some("0.3.1"), "0.3.1")
    );
    assert!(!lock.dirty, "nothing to write");
    drop(first);
}

/// Go's `TestRejectTamperedMetadata`: a release whose protocol, engines or checksum changed fails the index signature.
#[test]
fn a_release_changed_after_signing_is_refused() {
    let reg = registry("metadata", &[("0.3.0", 2, &["9"])], "");
    let m = modules(&reg);
    let index = reg.root.join("doze/valkey/index.yaml");
    let signed = std::fs::read_to_string(&index).unwrap();
    let sha = signed
        .lines()
        .find_map(|l| l.trim().strip_prefix("sha256: "))
        .unwrap()
        .to_string();
    let changes = [
        ("protocol: 2", "protocol: 3".to_string()),
        ("- \"9\"", "- \"8\"".to_string()),
        (sha.as_str(), "0".repeat(64)),
    ];
    for (from, to) in changes {
        std::fs::write(&index, signed.replace(from, &to)).unwrap();
        let err = m.resolve(&valkey(), &[], &mut Lock::default()).unwrap_err();
        assert!(
            err.starts_with("module index for doze/valkey"),
            "{from}: {err}"
        );
    }
}

/// Go's `TestLockMissingFile`: a lock that is not there reads as an empty one.
#[test]
fn a_missing_lock_reads_as_empty() {
    let gone = std::env::temp_dir().join(format!(
        "doze-fetch-nolock-{}/doze.lock",
        std::process::id()
    ));
    let l = Lock::load(&gone).unwrap();
    assert!(l.modules.is_empty() && l.doze.is_empty() && !l.dirty);
}

/// Audit F1, minor 2: with the first registry down, a relative artifact address is fetched from the base that served
/// the index.
#[test]
fn a_relative_artifact_comes_from_the_base_that_answered() {
    let reg = registry("secondbase", &[("0.3.0", 2, &["9"])], "");
    let mut m = modules(&reg);
    m.registry
        .bases
        .insert(0, crate::registry::Base::Http("http://127.0.0.1:1".into()));
    let r = m.resolve(&valkey(), &[], &mut Lock::default()).unwrap();
    assert_eq!(r.version, "0.3.0");
}
