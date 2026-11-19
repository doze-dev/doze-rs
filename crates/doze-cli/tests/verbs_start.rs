//! The verbs Go started the daemon for (`startDaemon`): with doze stopped, `wake` and `reset` start the built doze as the
//! stack's daemon (D1's `__daemon`) under the stack lock, say so, then act; `down` leaves nothing.
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

/// Stops the daemon a test started (by its exact pid, while it lives) however the test ends.
struct Reap(Option<u32>);

impl Drop for Reap {
    fn drop(&mut self) {
        if let Some(pid) = self.0.filter(|p| doze_os::process::alive(*p)) {
            let _ = doze_os::process::terminate(pid);
        }
    }
}

/// `wake` and `reset` with doze stopped start the daemon first, as Go's did (`startDaemon`), and say so on stderr; a
/// reset then removes the data folder, so the next wake starts fresh (Go's suite: `TestSetupRunsOncePerDataFolder`).
#[test]
fn a_verb_starts_the_daemon_when_doze_is_stopped() {
    if !std::path::Path::new("/bin/sh").exists() || !setup::sockets() {
        return;
    }
    let (sb, mut host, _) = setup::prepare(&["project bare".to_owned()]);
    host.daemon_exe = Some(std::path::PathBuf::from(env!("CARGO_BIN_EXE_doze")));
    let hcl = format!(
        "stack = \"shop\"\nhome = \"{}\"\n\nservice \"mail\" {{\n  run = \"echo up >> $DATA/ran && exec sleep 600\"\n}}\n",
        sb.home().display()
    );
    std::fs::write(sb.project().join("doze.hcl"), hcl).unwrap();
    let key = doze_projects::StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"));
    let doze = |args: &[&str]| fixture::run(&sb, &host, Terminals::default(), false, args);
    let ran = || {
        let data = sb.home().join("projects");
        let mut lines = 0;
        for e in walk(&data) {
            if e.file_name().is_some_and(|n| n == "ran") {
                lines += std::fs::read_to_string(&e)
                    .unwrap_or_default()
                    .lines()
                    .count();
            }
        }
        lines
    };
    let wake = doze(&["wake", "mail"]);
    let _reap = Reap(doze_projects::daemon_pid(&key).map(|p| p.pid));
    assert_eq!(wake.code, 0, "{}\n{}", wake.stdout, wake.stderr);
    assert!(
        wake.stderr
            .contains("started the doze daemon for doze wake"),
        "{}",
        wake.stderr
    );
    assert_eq!(ran(), 1, "mail ran once");
    let down = doze(&["down"]);
    assert_eq!(down.code, 0, "{}", down.stderr);
    // Stopped again: the reset starts the daemon, which deletes the data folder.
    let reset = doze(&["reset", "-y", "mail"]);
    let _reap2 = Reap(doze_projects::daemon_pid(&key).map(|p| p.pid));
    assert_eq!(reset.code, 0, "{}\n{}", reset.stdout, reset.stderr);
    assert!(
        reset
            .stderr
            .contains("started the doze daemon for doze reset"),
        "{}",
        reset.stderr
    );
    assert_eq!(ran(), 0, "the data folder is gone");
    let down = doze(&["down"]);
    assert_eq!(down.code, 0, "{}", down.stderr);
    assert!(!key.socket().exists() && !key.pid_file().exists());
}

/// Every file under `dir`.
fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}
