//! `doze update`'s hand-over, for real: `up` starts the built doze as the stack's daemon, `update` (brew faked by the
//! test host, the new doze the built one on the PATH) stops that daemon, starts a new one from the doze on the PATH with
//! the recorded doze.hcl, and wakes the app again. `down` then stops the new one.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/fixture.rs"]
mod fixture;
#[path = "support/setup.rs"]
mod setup;

use doze_cli::Terminals;
use std::os::unix::fs::PermissionsExt;

/// Stops a daemon the test started (by its exact pid, while it lives) however the test ends.
struct Reap(Vec<u32>);

impl Drop for Reap {
    fn drop(&mut self) {
        for pid in self
            .0
            .iter()
            .copied()
            .filter(|p| doze_os::process::alive(*p))
        {
            let _ = doze_os::process::terminate(pid);
        }
    }
}

#[test]
fn update_moves_a_running_stack_to_the_new_doze_and_wakes_what_was_awake() {
    if !setup::sockets() {
        return;
    }
    let (sb, mut host, _) = setup::prepare(&["registry".to_owned(), "project bare".to_owned()]);
    let built = std::path::PathBuf::from(env!("CARGO_BIN_EXE_doze"));
    host.daemon_exe = Some(built.clone());
    let bin = sb.root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("brew"), "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(bin.join("brew"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::os::unix::fs::symlink(&built, bin.join("doze")).unwrap();
    host.env.insert("PATH".into(), bin.display().to_string());
    let hcl = format!(
        "stack = \"shop\"\nhome = \"{}\"\n\napp \"web\" {{\n  run = \"exec sleep 600\"\n}}\n",
        sb.home().display()
    );
    std::fs::write(sb.project().join("doze.hcl"), hcl).unwrap();
    let key = doze_projects::StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"));
    let up = fixture::run(&sb, &host, Terminals::default(), false, &["up"]);
    let old = doze_projects::daemon_pid(&key).map(|p| p.pid);
    let mut reap = Reap(old.into_iter().collect());
    assert_eq!(up.code, 0, "{}\n{}", up.stdout, up.stderr);
    let upd = fixture::run(
        &sb,
        &host,
        Terminals::default(),
        true,
        &["--json", "update"],
    );
    let new = doze_projects::daemon_pid(&key).map(|p| p.pid);
    reap.0.extend(new);
    assert_eq!(upd.code, 0, "{}\n{}", upd.stdout, upd.stderr);
    let v: serde_json::Value = serde_json::from_str(&upd.stdout).unwrap();
    let moved = &v["data"]["stacks"][0];
    assert_eq!(moved["stack"], "shop", "{}", upd.stdout);
    assert_eq!(moved["moved"], true, "{}", upd.stdout);
    assert_eq!(moved["detail"], "woken again: web", "{}", upd.stdout);
    assert!(
        old.is_some_and(|p| !doze_os::process::alive(p)),
        "the old daemon is gone"
    );
    assert!(
        new.is_some_and(doze_os::process::alive) && new != old,
        "a new daemon runs"
    );
    let down = fixture::run(&sb, &host, Terminals::default(), false, &["down"]);
    assert_eq!(down.code, 0, "{}", down.stderr);
}

/// A stack whose daemon cannot be identified (its pid file names none) keeps the old doze, named with why: exit 11.
#[test]
fn a_stack_that_cannot_be_moved_is_named_and_the_update_is_partial() {
    if !setup::sockets() {
        return;
    }
    let (sb, mut host, _d) = setup::prepare(&[
        "registry".to_owned(),
        "project bare".to_owned(),
        "daemon empty".to_owned(),
    ]);
    let bin = sb.root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("brew"), "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(bin.join("brew"), std::fs::Permissions::from_mode(0o755)).unwrap();
    host.env.insert("PATH".into(), bin.display().to_string());
    host.daemon_exe = Some(std::path::PathBuf::from(env!("CARGO_BIN_EXE_doze")));
    let key = doze_projects::StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"));
    let config = sb.project().join("doze.hcl").display().to_string();
    doze_projects::update(key.dir(), |p| p.config = config).unwrap();
    let upd = fixture::run(&sb, &host, Terminals::default(), true, &["update"]);
    assert_eq!(upd.code, 11, "{}\n{}", upd.stdout, upd.stderr);
    assert!(
        upd.stderr.contains("1 stack(s) keep doze"),
        "{}",
        upd.stderr
    );
    assert!(
        upd.stderr.contains("its pid file names no daemon"),
        "{}",
        upd.stderr
    );
}

/// A new doze that does not run stops nothing: every stack stays on the old one, named, exit 11.
#[test]
fn a_new_doze_that_does_not_run_stops_nothing() {
    if !setup::sockets() {
        return;
    }
    let (sb, mut host, _) = setup::prepare(&["registry".to_owned(), "project bare".to_owned()]);
    host.daemon_exe = Some(std::path::PathBuf::from(env!("CARGO_BIN_EXE_doze")));
    let bin = sb.root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for (name, body) in [("brew", "#!/bin/sh\n"), ("doze", "#!/bin/sh\nexit 1\n")] {
        std::fs::write(bin.join(name), body).unwrap();
        std::fs::set_permissions(bin.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    host.env.insert("PATH".into(), bin.display().to_string());
    let hcl = format!(
        "stack = \"shop\"\nhome = \"{}\"\n\napp \"web\" {{\n  run = \"exec sleep 600\"\n}}\n",
        sb.home().display()
    );
    std::fs::write(sb.project().join("doze.hcl"), hcl).unwrap();
    let key = doze_projects::StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"));
    let up = fixture::run(&sb, &host, Terminals::default(), false, &["up"]);
    let old = doze_projects::daemon_pid(&key).map(|p| p.pid);
    let _reap = Reap(old.into_iter().collect());
    assert_eq!(up.code, 0, "{}", up.stderr);
    let upd = fixture::run(&sb, &host, Terminals::default(), true, &["update"]);
    assert_eq!(upd.code, 11, "{}\n{}", upd.stdout, upd.stderr);
    assert!(
        upd.stderr.contains("does not run as this doze")
            && upd.stderr.contains("nothing was stopped"),
        "{}",
        upd.stderr
    );
    assert!(
        old.is_some_and(doze_os::process::alive),
        "the old daemon was stopped"
    );
    let down = fixture::run(&sb, &host, Terminals::default(), false, &["down"]);
    assert_eq!(down.code, 0, "{}", down.stderr);
}

/// An older doze on the PATH (one that runs `version` but cannot answer `--json version`, as the Go doze) is never
/// handed a stack: nothing is stopped, exit 11.
#[test]
fn an_older_doze_on_the_path_is_never_handed_a_stack() {
    if !setup::sockets() {
        return;
    }
    let (sb, mut host, _) = setup::prepare(&["registry".to_owned(), "project bare".to_owned()]);
    host.daemon_exe = Some(std::path::PathBuf::from(env!("CARGO_BIN_EXE_doze")));
    let bin = sb.root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for (name, body) in [
        ("brew", "#!/bin/sh\n"),
        (
            "doze",
            "#!/bin/sh\n[ \"$1\" = --json ] && { echo 'Error: unknown flag: --json' >&2; exit 1; }\necho 'doze 0.1.1'\n",
        ),
    ] {
        std::fs::write(bin.join(name), body).unwrap();
        std::fs::set_permissions(bin.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    host.env.insert("PATH".into(), bin.display().to_string());
    let hcl = format!(
        "stack = \"shop\"\nhome = \"{}\"\n\napp \"web\" {{\n  run = \"exec sleep 600\"\n}}\n",
        sb.home().display()
    );
    std::fs::write(sb.project().join("doze.hcl"), hcl).unwrap();
    let key = doze_projects::StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"));
    let up = fixture::run(&sb, &host, Terminals::default(), false, &["up"]);
    let old = doze_projects::daemon_pid(&key).map(|p| p.pid);
    let _reap = Reap(old.into_iter().collect());
    assert_eq!(up.code, 0, "{}", up.stderr);
    let upd = fixture::run(&sb, &host, Terminals::default(), true, &["update"]);
    assert_eq!(upd.code, 11, "{}\n{}", upd.stdout, upd.stderr);
    assert!(
        upd.stderr.contains("does not run as this doze")
            && upd.stderr.contains("nothing was stopped"),
        "{}",
        upd.stderr
    );
    assert!(
        old.is_some_and(doze_os::process::alive),
        "the old daemon was stopped"
    );
    let down = fixture::run(&sb, &host, Terminals::default(), false, &["down"]);
    assert_eq!(down.code, 0, "{}", down.stderr);
}
