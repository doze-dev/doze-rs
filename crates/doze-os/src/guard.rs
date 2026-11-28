//! A tripwire between tests and the real machine: a test must never raise an administrator prompt, run sudo, ask
//! macOS's trust dialog, or write the person's real doze home. Each place that does any of those calls in here first,
//! and under a test it panics, loudly and with what it was about to do, instead of doing it.
//!
//! What is under a test:
//! - a test binary, told by where it is: cargo puts every unit and integration test in `<target>/<profile>/deps/`,
//!   and nothing doze ships runs from there;
//! - a program a test started (the doze daemon, `doze-names`, the app's doze hand): it runs from `<target>/<profile>/`,
//!   not `deps/`, but under a cargo-started process it inherits cargo's `CARGO_MANIFEST_DIR`. So a program inside a
//!   cargo target folder with that variable set is under a test too, and so is `cargo run` of one (a by-hand run that
//!   means to change the machine says `DOZE_REAL_MACHINE=1`); the same program run from a shell, the app bundle or a
//!   copy elsewhere is not;
//! - anything with `DOZE_UNDER_TEST=1`, for a harness that wants to say so outright.
//!
//! The end-to-end runs that change the machine on purpose say so: `DOZE_REAL_MACHINE=1`, or for sudo, the
//! administrator prompt and macOS's dialog the variables their scripts already set (`DOZE_NAMES_ROOT_E2E=1`,
//! `DOZE_ADMIN_E2E=1`). Those two never open the real doze home: they run with a `DOZE_HOME` of their own, and only
//! `DOZE_REAL_MACHINE=1` lets a test write the real one.
//!
//! Why: on 2026-11-28 a new root appeared in the owner's System keychain and his ~/.doze/ca was remade while gates ran;
//! whatever did it, no test may be able to.

use std::path::{Path, PathBuf};

/// The variable a harness sets to say that what it starts is under a test.
pub const UNDER_TEST: &str = "DOZE_UNDER_TEST";

/// The variable that lets a test reach the real machine and home on purpose (end-to-end scripts only).
pub const REAL_MACHINE: &str = "DOZE_REAL_MACHINE";

/// A doze home the guard protects as it does the real one: for the guard's own tests, which prove a spawned program
/// stops before writing it without going near the real home.
pub const GUARDED_HOME: &str = "DOZE_GUARDED_HOME";

fn set(name: &str) -> bool {
    std::env::var(name).as_deref() == Ok("1")
}

/// Whether this process is under a test: a cargo test binary, a program one started, or a harness said so.
pub fn under_test() -> bool {
    if set(UNDER_TEST) {
        return true;
    }
    let cargo = std::env::var_os("CARGO_MANIFEST_DIR").is_some();
    std::env::current_exe().is_ok_and(|exe| {
        in_deps(&exe) || (cargo && in_target(&exe, std::env::var_os("CARGO_TARGET_DIR")))
    })
}

/// A test binary's place: a `deps` folder inside a cargo target folder.
fn in_deps(exe: &Path) -> bool {
    exe.parent()
        .and_then(Path::file_name)
        .is_some_and(|n| n == "deps")
        && in_target(exe, std::env::var_os("CARGO_TARGET_DIR"))
}

/// Whether `exe` is inside a cargo target folder: one named `target` (or `target-<something>`), or `CARGO_TARGET_DIR`.
fn in_target(exe: &Path, target_dir: Option<std::ffi::OsString>) -> bool {
    let named = exe.components().any(|c| {
        c.as_os_str()
            .to_str()
            .is_some_and(|s| s == "target" || s.starts_with("target-"))
    });
    named
        || target_dir
            .filter(|d| !d.is_empty())
            .is_some_and(|d| canonical(exe).starts_with(canonical(Path::new(&d))))
}

/// Stops a test that is about to change the real machine (`what`: "sudo", "the administrator prompt", …).
///
/// # Panics
/// Under a test, unless a machine-changing end-to-end run asked for it.
#[allow(clippy::panic)] // WHY: only a test gets here, and it must stop loudly before it changes the machine
pub fn real_machine(what: &str) {
    if under_test()
        && ![REAL_MACHINE, "DOZE_NAMES_ROOT_E2E", "DOZE_ADMIN_E2E"]
            .iter()
            .any(|v| set(v))
    {
        panic!(
            "a test tried to reach the real machine ({what}); tests must hand privileged work to a recorder. \
             ({REAL_MACHINE}=1 is for the end-to-end scripts only.)"
        );
    }
}

/// The homes a test must not write: the person's real doze home (`~/.doze` of this uid, from the user database,
/// whatever `HOME` says) and [`GUARDED_HOME`]'s.
fn guarded_homes() -> Vec<PathBuf> {
    let mut homes: Vec<PathBuf> = crate::privilege::ids()
        .and_then(|(uid, _)| crate::privilege::home_of(uid))
        .map(|h| h.join(".doze"))
        .into_iter()
        .collect();
    if let Some(h) = std::env::var_os(GUARDED_HOME).filter(|h| !h.is_empty()) {
        homes.push(PathBuf::from(h));
    }
    homes
}

/// `p` with symbolic links resolved as far as it exists: the nearest ancestor that exists canonicalized, the rest put
/// back. So a linked `~/.doze`, or a path into it that is not made yet, is compared by where it really is.
fn canonical(p: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut at = p;
    loop {
        if let Ok(real) = std::fs::canonicalize(at) {
            return rest.iter().rev().fold(real, |acc, part| acc.join(part));
        }
        match (at.parent(), at.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                at = parent;
            }
            _ => return p.to_path_buf(),
        }
    }
}

/// Stops a test that is about to write under the person's real doze home (`what`: "make the certificate authority").
///
/// # Panics
/// Under a test, when `path` is in the real `~/.doze` (or [`GUARDED_HOME`]), unless `DOZE_REAL_MACHINE=1`.
#[allow(clippy::panic)] // WHY: only a test gets here, and it must stop loudly before it writes the real home
pub fn real_home_write(path: &Path, what: &str) {
    if !under_test() || set(REAL_MACHINE) {
        return;
    }
    let path = canonical(path);
    if let Some(home) = guarded_homes()
        .iter()
        .find(|h| path.starts_with(canonical(h)))
    {
        panic!(
            "a test tried to {what} in the real doze home ({}, which is {}); give it a DOZE_HOME of its own",
            path.display(),
            home.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_test_binary_is_told_by_its_folder() {
        assert!(under_test(), "this very test runs from target/*/deps");
        assert!(in_deps(Path::new("/x/target/debug/deps/doze_cli-0123")));
        assert!(in_deps(Path::new("/x/target-aws/debug/deps/doze_cli-0123")));
        assert!(!in_deps(Path::new("/x/target/debug/doze")));
        assert!(!in_deps(Path::new("/usr/local/bin/doze")));
        assert!(!in_deps(Path::new("/opt/deps/doze")));
        // A program a test started: in a target folder (named, or CARGO_TARGET_DIR), not in deps.
        assert!(in_target(Path::new("/x/target/debug/doze"), None));
        assert!(in_target(
            Path::new("/scratch/build/debug/doze"),
            Some("/scratch/build".into())
        ));
        assert!(!in_target(
            Path::new("/Applications/Doze.app/Contents/MacOS/doze"),
            None
        ));
        assert!(!in_target(
            Path::new("/usr/local/bin/doze"),
            Some("".into())
        ));
    }

    #[test]
    fn a_test_cannot_reach_the_real_machine_or_home() {
        if set(REAL_MACHINE) {
            return;
        }
        if !["DOZE_NAMES_ROOT_E2E", "DOZE_ADMIN_E2E"]
            .iter()
            .any(|v| set(v))
        {
            assert!(std::panic::catch_unwind(|| real_machine("sudo")).is_err());
        }
        for home in guarded_homes() {
            assert!(
                std::panic::catch_unwind(|| real_home_write(&home.join("ca"), "make the CA"))
                    .is_err()
            );
        }
        real_home_write(&std::env::temp_dir().join("elsewhere"), "write elsewhere");
    }

    /// A linked doze home is compared by where it really is, and so is a path in it not made yet.
    #[cfg(unix)]
    #[test]
    fn a_link_does_not_get_past_the_guard() {
        let base = std::env::temp_dir().join(format!("doze-guard-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("real")).unwrap();
        std::os::unix::fs::symlink(base.join("real"), base.join("link")).unwrap();
        let real = canonical(&base.join("real"));
        assert_eq!(
            canonical(&base.join("link").join("ca").join("x")),
            real.join("ca").join("x")
        );
        assert!(canonical(&base.join("link/ca")).starts_with(canonical(&base.join("real"))));
        let _ = std::fs::remove_dir_all(&base);
    }
}
