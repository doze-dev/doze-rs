use super::*;

pub(crate) fn manifest(versions: &[&str]) -> Manifest {
    Manifest {
        engine: "x".into(),
        category: String::new(),
        title: String::new(),
        tagline: String::new(),
        versions: versions.iter().map(|v| v.to_string()).collect(),
        exact_dots: 1,
        default_port: 5432,
        boot_budget_ms: None,
        supervised: true,
        backend_socket: None,
        caps: 0,
        schema: Default::default(),
        example: String::new(),
        console: None,
        actions: vec![],
        targets: vec![],
        description: None,
        homepage: None,
        source: None,
        example_label: None,
        default_version: None,
        client: None,
        detect: None,
    }
}

#[test]
fn what_the_config_needs_is_what_the_manifest_says() {
    assert_eq!(
        info(&manifest(&["16", "17"])),
        ModuleInfo {
            versionless: false,
            default_port: 5432,
            supervised: true
        }
    );
    assert!(info(&manifest(&[])).versionless);
}

#[test]
fn a_version_is_served_when_it_is_a_major_or_under_one() {
    let m = manifest(&["16", "17"]);
    assert!(supports("pg", &m, "17").is_ok());
    assert!(supports("pg", &m, "16.4").is_ok());
    assert!(
        supports("pg", &m, "").is_ok(),
        "no version: the module's default"
    );
    let e = supports("pg", &m, "170").unwrap_err();
    assert!(e.contains("serves 16, 17"), "{e}");
    assert!(supports("pg", &m, "1").is_err());
    assert!(
        supports("v", &manifest(&[]), "9").is_ok(),
        "versionless takes what doze-config allows"
    );
}

#[test]
fn the_locks_engine_pins_are_carried_as_the_protocol_carries_them() {
    let d = std::env::temp_dir().join(format!("doze-catalog-pins-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let lock = d.join("doze.lock");
    std::fs::write(
        &lock,
        "engines:\n  postgres:\n    \"17\":\n      resolved: 17.2.0\n      source: mirror\n      hashes:\n        t1: sha256:aa\n",
    )
    .unwrap();
    let pins = lock_pins(&lock);
    assert_eq!(pins.len(), 1);
    let p = &pins[0];
    assert_eq!(
        (p.engine.as_str(), p.spec.as_str(), p.resolved.as_str()),
        ("postgres", "17", "17.2.0")
    );
    assert_eq!(p.hashes.get("t1").map(String::as_str), Some("sha256:aa"));
    assert!(lock_pins(&d.join("none")).is_empty(), "no lock, no pins");
    let _ = std::fs::remove_dir_all(&d);
}

/// The D1 audit's lock writer: instances recording pins at once each keep theirs (one writer at a time).
#[test]
fn pins_recorded_at_once_are_all_kept() {
    let dir = std::env::temp_dir().join(format!("doze-pins-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_default();
    let path = dir.join(doze_fetch::lock::FILE);
    let writers: Vec<_> = (0..8)
        .map(|n| {
            let path = path.clone();
            std::thread::spawn(move || {
                let pin = doze_module_proto::msg::run::Pin {
                    engine: format!("e{n}"),
                    spec: "1".into(),
                    resolved: "1.0.0".into(),
                    hashes: std::collections::BTreeMap::from([(
                        "t".to_string(),
                        format!("sha256:{n}"),
                    )]),
                };
                record_pins(&path, &[pin]);
            })
        })
        .collect();
    for w in writers {
        assert!(w.join().is_ok());
    }
    let lock = doze_fetch::lock::Lock::load(&path).ok();
    let engines: Vec<String> = lock
        .map(|l| l.engines.keys().cloned().collect())
        .unwrap_or_default();
    assert_eq!(engines.len(), 8, "{engines:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
