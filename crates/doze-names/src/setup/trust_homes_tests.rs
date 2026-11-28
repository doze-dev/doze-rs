use super::*;
use crate::setup::shell_quote;

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("doze-names-homes-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Two doze homes on one machine, each with its own root: whatever one home does to its authority (a rotation with its
/// root unreadable or absent, the last `trust --remove`, an uninstall's cleanup) names only its own fingerprints, never
/// the other's, though every doze root shares one common name. (Until 2026-11-28 these took every "doze local CA" the
/// System keychain held.)
#[test]
fn one_home_never_touches_another_homes_root() {
    let (a, b) = (scratch("home-a"), scratch("home-b"));
    let ca_b = Ca::load_or_create(&ca_dir(&b)).unwrap();
    let fp_b = ca_b.fingerprint();
    let theirs = |p: &str| p.contains(&fp_b) || p.contains(&b.display().to_string());
    // B's root is in the machine's store, where the old rotation looked when A's root could not be read (and took every
    // "doze local CA" it found): a rotation that took it again would fail here.
    crate::trust_store::pretend_store_holds(Some(vec![fp_b.clone()]));
    for broken in [false, true] {
        let dir = ca_dir(&a);
        let _ = std::fs::remove_dir_all(&dir);
        if broken {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("root.crt"), "not a certificate").unwrap();
        }
        let rotated = Ca::rotate(&dir).unwrap();
        assert!(
            rotated.previous().is_empty(),
            "a rotation of an unreadable or absent root starts fresh: {:?}",
            rotated.previous()
        );
        for os in [Os::Mac, Os::Windows, Os::Linux] {
            let req = TrustRequest {
                ca: Ca::probe(&dir).unwrap(),
                ..TrustRequest::default()
            };
            let p = untrust_plan(os, &req).unwrap();
            let all = format!("{}{}{}", p.script, p.as_user, p.as_user_before);
            assert!(!theirs(&all), "{os:?}: {all}");
            assert!(
                !all.contains("delete-certificate -c") && !all.contains("doze local CA"),
                "{os:?}: never by name: {all}"
            );
            if os == Os::Linux {
                // One bundle file for every home: it goes only while it is a copy of A's root.
                let a_root = shell_quote(&rotated.root_path().display().to_string());
                assert!(
                    all.starts_with(&format!("if cmp -s {a_root} ")) && !all.contains("\nrm -f"),
                    "{os:?}: only while the bundle is A's: {all}"
                );
            } else {
                assert!(
                    all.contains(&rotated.fingerprint()),
                    "{os:?}: A's own root by fingerprint: {all}"
                );
            }
            let (root, before) = cleanup_script(os, &a);
            let all = format!("{}{before}", root.join("\n"));
            assert!(!theirs(&all), "{os:?} uninstall: {all}");
            assert!(
                !all.contains("delete-certificate -c") && !all.contains("doze local CA"),
                "never by name: {all}"
            );
        }
    }
    // With A's authority gone altogether, its removal names no root at all.
    let _ = std::fs::remove_dir_all(ca_dir(&a));
    let req = TrustRequest::default();
    for os in [Os::Mac, Os::Windows, Os::Linux] {
        assert_eq!(
            untrust_plan(os, &req),
            None,
            "{os:?}: nothing is removed by name"
        );
        assert!(cleanup_script(os, &a).0.is_empty(), "{os:?}");
    }
    crate::trust_store::pretend_store_holds(None);
    for d in [a, b] {
        let _ = std::fs::remove_dir_all(d);
    }
}
