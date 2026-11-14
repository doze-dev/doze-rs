//! The child's own decisions: what a module may ask for, and how a pin reaches `doze.lock` when several children save
//! it at once.

use super::*;
use crate::lock::Pin;
use crate::modules::tests::{majors, modules, registry, valkey};

fn place(m: Modules) -> Place {
    Place {
        mirror: Mirror {
            root: "file:///nowhere".into(),
            home: m.home.clone(),
            key: None,
        },
        modules: m,
    }
}

/// Audit F1, M1: an ensure names a real platform, or this machine's; anything else becomes no cache folder.
#[test]
fn an_ensure_for_an_unknown_platform_is_refused() {
    let reg = registry("triple", &[("0.3.0", 2, &["9"])], "");
    let p = place(modules(&reg));
    let ask = |target: &str| Request::Ensure {
        engine: "valkey".into(),
        full: "9.1.1".into(),
        target: target.into(),
        expected_sha: None,
    };
    let r = serve(&ask("sparc-sun-solaris"), &p);
    assert!(
        r.error
            .as_deref()
            .is_some_and(|e| e.contains("not a platform")),
        "{r:?}"
    );
    // A real platform passes the check (and then fails at the unreachable mirror).
    let r = serve(&ask("x86_64-unknown-linux-gnu"), &p);
    assert!(
        r.error.as_deref().is_some_and(|e| !e.contains("refused")),
        "{r:?}"
    );
}

/// Audit F1, minor 10: a module request that persists writes the pin and says so once; a read command does neither.
#[test]
fn a_persisted_module_request_pins_it_and_says_so_once() {
    let reg = registry("persist", &[("0.3.0", 2, &["9"])], "");
    let p = place(modules(&reg));
    let lock = reg.home.join("doze.lock");
    let ask = |persist: bool| Request::Module {
        source: valkey().to_string(),
        majors: majors(&["9"]),
        lock: Some(lock.clone()),
        persist,
    };
    let r = serve(&ask(false), &p);
    assert_eq!((r.error.as_deref(), r.notes.len()), (None, 0), "{r:?}");
    assert!(!lock.exists(), "a read command never writes the lock");
    let r = serve(&ask(true), &p);
    assert_eq!(r.version, "0.3.0");
    assert!(
        r.notes
            .iter()
            .any(|n| n.starts_with("pinned doze/valkey 0.3.0 in doze.lock")),
        "{r:?}"
    );
    assert_eq!(
        Lock::load(&lock).unwrap().modules["doze/valkey"].version,
        "0.3.0"
    );
    let again = serve(&ask(true), &p);
    assert!(again.notes.is_empty(), "{again:?}");
}

/// Audit F1, M3: children resolving different modules at once all save into one `doze.lock`; no pin is lost.
///
/// Without the lock this fails, but not first on a lost update: the threads share one pid, so they write the same
/// temp file beside `doze.lock` and one rename finds it already moved (an error). Separate children would lose pins
/// instead. Either way only the lock makes every save land.
#[test]
fn concurrent_saves_keep_every_pin() {
    let reg = registry("concurrent", &[("0.3.0", 2, &["9"])], "");
    let p = place(modules(&reg));
    std::fs::create_dir_all(&reg.home).unwrap();
    let lock = reg.home.join("doze.lock");
    let names: Vec<String> = (0..12).map(|i| format!("m{i}")).collect();
    std::thread::scope(|s| {
        for n in &names {
            let (p, lock) = (&p, &lock);
            s.spawn(move || {
                let src = Source::parse(n).unwrap();
                let pin = Pin {
                    version: "1.0.0".into(),
                    protocol: 2,
                    ..Pin::default()
                };
                assert!(save_pin(p, lock, &src, Some(&pin)).unwrap());
            });
        }
    });
    let saved = Lock::load(&lock).unwrap();
    for n in &names {
        assert!(saved.modules.contains_key(&format!("doze/{n}")), "{n} lost");
    }
}

/// F1 audit 3, minor 1: when the lock folder cannot be made, the note names it (`<home>/locks`), and the request
/// still succeeds without the pin.
#[test]
fn a_lock_folder_that_cannot_be_made_is_named() {
    let reg = registry("lockdir", &[("0.3.0", 2, &["9"])], "");
    let p = place(modules(&reg));
    std::fs::create_dir_all(&p.modules.home).unwrap();
    let locks = p.modules.home.join("locks");
    std::fs::write(&locks, "a file, not a folder").unwrap();
    let r = serve(
        &Request::Module {
            source: valkey().to_string(),
            majors: majors(&["9"]),
            lock: Some(reg.home.join("doze.lock")),
            persist: true,
        },
        &p,
    );
    assert_eq!((r.error.as_deref(), r.version.as_str()), (None, "0.3.0"));
    let want = format!("taking the doze.lock lock in {}", locks.display());
    assert!(r.notes.iter().any(|n| n.contains(&want)), "{r:?}");
}

/// Audit F1, minor 4 (Go's `loadLock`): a lock written by a newer doze is said, once per resolve that reads it.
#[test]
fn a_lock_from_a_newer_doze_is_said() {
    let reg = registry("newerlock", &[("0.3.0", 2, &["9"])], "");
    let p = place(modules(&reg));
    std::fs::create_dir_all(&reg.home).unwrap();
    let lock = reg.home.join("doze.lock");
    std::fs::write(&lock, "doze: 9.9.9\n").unwrap();
    let r = serve(
        &Request::Module {
            source: "valkey".into(),
            majors: vec![],
            lock: Some(lock),
            persist: false,
        },
        &p,
    );
    assert!(
        r.notes.first().is_some_and(|n| n
            == "doze.lock was written by doze 9.9.9, newer than this doze (0.3.0); if a module will not start, update doze"),
        "{r:?}"
    );
}

/// F1 audit 2, major 1: with a real platform, an engine or version that is a path is refused as a name.
#[test]
fn an_ensure_whose_names_are_paths_is_refused() {
    let reg = registry("names", &[("0.3.0", 2, &["9"])], "");
    let p = place(modules(&reg));
    for (engine, full) in [("../x", "9.1.1"), ("valkey", "/abs"), ("valkey", "../../x")] {
        let r = serve(
            &Request::Ensure {
                engine: engine.into(),
                full: full.into(),
                target: "x86_64-unknown-linux-gnu".into(),
                expected_sha: None,
            },
            &p,
        );
        assert!(
            r.error
                .as_deref()
                .is_some_and(|e| e.contains("not a plain name")),
            "{engine} {full}: {r:?}"
        );
    }
}
