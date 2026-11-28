//! `doze down` with the real daemon: every service gets to finish what it does on its way out, what others need is
//! stopped after them, and a service that had to be killed is said to have been (the 2026-11-28 larder `down`).
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

/// Stops the daemon if a test failed before its `down`.
struct Reap(Option<u32>);

impl Drop for Reap {
    fn drop(&mut self) {
        if let Some(pid) = self.0.filter(|p| doze_os::process::alive(*p)) {
            let _ = doze_os::process::terminate(pid);
        }
    }
}

/// `down` lets each service finish what it does on its way out, and stops what others need last. `worker` needs
/// `broker` (`after`); on SIGTERM it takes 1.5 s, then notes whether `broker` is still up (its file is there) and only
/// then exits. The 2026-11-28 larder `down` stopped a consumer together with its broker: the consumer waited on its
/// goodbye to the broker for its whole grace and was killed.
#[test]
fn down_waits_for_a_slow_stop_and_stops_dependants_first() {
    if !std::path::Path::new("/bin/sh").exists() || !setup::sockets() {
        return;
    }
    let (sb, mut host, _) = setup::prepare(&["project bare".to_owned()]);
    host.daemon_exe = Some(std::path::PathBuf::from(env!("CARGO_BIN_EXE_doze")));
    let up_file = sb.root.join("broker.up");
    let seen = sb.root.join("worker.seen");
    let (up_file, seen) = (up_file.display(), seen.display());
    let hcl = format!(
        r#"stack = "shop"
home = "{home}"

app "broker" {{
  run = "trap 'rm -f {up_file}; exit 0' TERM; touch {up_file}; while :; do sleep 0.1; done"
}}

app "worker" {{
  after = [app.broker]
  run   = "trap 'sleep 1.5; if [ -e {up_file} ]; then echo broker-up > {seen}; else echo broker-gone > {seen}; fi; exit 0' TERM; while :; do sleep 0.1; done"
}}
"#,
        home = sb.home().display(),
    );
    std::fs::write(sb.project().join("doze.hcl"), hcl).unwrap();
    let key = doze_projects::StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"));
    let up = fixture::run(&sb, &host, Terminals::default(), false, &["up"]);
    let pid = doze_projects::daemon_pid(&key).map(|p| p.pid);
    let _reap = Reap(pid);
    assert_eq!(up.code, 0, "{}\n{}", up.stdout, up.stderr);
    assert!(up.stdout.contains("✓ worker awake"), "{}", up.stdout);
    let began = std::time::Instant::now();
    let down = fixture::run(&sb, &host, Terminals::default(), false, &["--json", "down"]);
    let took = began.elapsed();
    assert_eq!(down.code, 0, "{}", down.stderr);
    let said = std::fs::read_to_string(sb.root.join("worker.seen")).unwrap_or_default();
    assert_eq!(
        said.trim(),
        "broker-up",
        "the worker finished its stop, with what it needs still up"
    );
    assert!(
        !sb.root.join("broker.up").exists(),
        "the broker was stopped after, by SIGTERM"
    );
    assert!(
        took >= std::time::Duration::from_millis(1500) && took < std::time::Duration::from_secs(8),
        "down waited for the slow stop, and killed nothing: {took:?}"
    );
    let log = std::fs::read_to_string(key.log_file()).unwrap_or_default();
    assert!(!log.contains("SIGKILL"), "{log}");
}

/// A service that will not stop is killed after its grace, and that is said: a warning naming it in the daemon's log
/// and the journal's sleep says it was killed. The larder valkey killed this way on 2026-11-28 lost its snapshot, and
/// nothing anywhere said it had been killed.
#[test]
fn a_kill_at_the_end_of_the_grace_is_said() {
    if !std::path::Path::new("/bin/sh").exists() || !setup::sockets() {
        return;
    }
    let (sb, mut host, _) = setup::prepare(&["project bare".to_owned()]);
    host.daemon_exe = Some(std::path::PathBuf::from(env!("CARGO_BIN_EXE_doze")));
    let hcl = format!(
        "stack = \"shop\"\nhome = \"{}\"\n\napp \"stubborn\" {{\n  run = \"trap '' TERM; while :; do sleep 0.1; done\"\n}}\n",
        sb.home().display()
    );
    std::fs::write(sb.project().join("doze.hcl"), hcl).unwrap();
    let key = doze_projects::StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"));
    let up = fixture::run(&sb, &host, Terminals::default(), false, &["up"]);
    let pid = doze_projects::daemon_pid(&key).map(|p| p.pid);
    let _reap = Reap(pid);
    assert_eq!(up.code, 0, "{}\n{}", up.stdout, up.stderr);
    let down = fixture::run(&sb, &host, Terminals::default(), false, &["--json", "down"]);
    assert_eq!(down.code, 0, "{}", down.stderr);
    let log = std::fs::read_to_string(key.log_file()).unwrap_or_default();
    assert!(
        log.lines().any(|l| l.contains(" warning: \"stubborn\" did not exit within 8s of being asked to stop; it was killed (SIGKILL)")),
        "{log}"
    );
    let journal = std::fs::read_to_string(key.run_dir().join("journal.jsonl")).unwrap_or_default();
    let sleep = journal
        .lines()
        .find(|l| l.contains("\"kind\":\"sleep\"") && l.contains("\"stubborn\""))
        .unwrap_or_default();
    assert!(
        sleep.contains("killed: stubborn did not exit within 8s"),
        "{journal}"
    );
}
