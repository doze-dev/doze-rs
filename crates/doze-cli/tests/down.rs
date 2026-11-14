//! `doze down` stops the daemon its pid file names (SIGTERM to that exact pid) only when that pid is still the daemon,
//! waits for it to go, and leaves no socket or pid file behind (proposal rule 8). Stand-in processes play the daemon
//! (in a group of its own, as the daemon's session is) and a stranger (not a group leader): the test's own children.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/fixture.rs"]
mod fixture;

use doze_cli::Terminals;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, ExitStatus};
use std::thread::JoinHandle;

fn stack(sb: &fixture::Sandbox) -> doze_projects::StackKey {
    std::fs::write(sb.project().join("doze.hcl"), "stack = \"shop\"\n").unwrap();
    let key = doze_projects::StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"));
    std::fs::create_dir_all(key.run_dir()).unwrap();
    key
}

/// A `sleep` (in a group of its own when `leader`), its pid in the pid file, reaped as it ends (as init reaps the
/// detached daemon: a zombie would still answer kill(pid, 0)).
fn stand_in(key: &doze_projects::StackKey, leader: bool) -> (u32, JoinHandle<ExitStatus>) {
    let mut cmd = Command::new("sleep");
    cmd.arg("30");
    if leader {
        cmd.process_group(0);
    }
    let mut child: Child = cmd.spawn().unwrap();
    let pid = child.id();
    std::fs::write(key.pid_file(), format!("{pid}\n")).unwrap();
    (pid, std::thread::spawn(move || child.wait().unwrap()))
}

fn run(sb: &fixture::Sandbox, args: &[&str]) -> fixture::Ran {
    let host = fixture::FakeHost::default();
    fixture::run(sb, &host, Terminals::default(), false, args)
}

#[test]
fn down_stops_the_daemon_by_its_pid_and_leaves_nothing() {
    let sb = fixture::Sandbox::new("down");
    let key = stack(&sb);
    let (pid, reaper) = stand_in(&key, true);
    // A socket file nobody listens on: what a daemon leaves when it is killed.
    std::fs::write(key.socket(), "").unwrap();
    let plan = run(&sb, &["down", "--dry-run"]);
    assert_eq!(plan.code, 0, "{}", plan.stderr);
    assert!(
        plan.stdout
            .contains(&format!("stop the daemon (pid {pid})")),
        "{}",
        plan.stdout
    );
    assert!(
        doze_os::process::alive(pid),
        "a dry run signalled the daemon"
    );
    let ran = run(&sb, &["--json", "down"]);
    if ran.code != 0 {
        let _ = doze_os::process::kill(pid);
    }
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let v: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    assert_eq!(v["data"]["stopped"], true, "{}", ran.stdout);
    assert_eq!(
        reaper.join().unwrap().signal(),
        Some(15),
        "not ended by SIGTERM"
    );
    assert!(!key.pid_file().exists());
    assert!(!key.socket().exists());
}

#[test]
fn a_stale_pid_file_naming_a_stranger_signals_nobody() {
    let sb = fixture::Sandbox::new("down-stranger");
    let key = stack(&sb);
    // Not a group leader: not a daemon doze started, though alive under the recorded pid.
    let (pid, reaper) = stand_in(&key, false);
    let plan = run(&sb, &["down", "--dry-run"]);
    let ran = run(&sb, &["down"]);
    let survived = doze_os::process::alive(pid);
    let _ = doze_os::process::kill(pid);
    reaper.join().unwrap();
    assert!(survived, "down signalled a process it did not start");
    assert!(plan.stdout.contains("pid file"), "{}", plan.stdout);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert!(
        ran.stdout.contains("removed a leftover pid file"),
        "{}",
        ran.stdout
    );
    assert!(
        ran.stdout.ends_with("doze is not running\n"),
        "{}",
        ran.stdout
    );
}

#[test]
fn a_pid_file_of_another_users_process_is_stale_not_a_wall() {
    let sb = fixture::Sandbox::new("down-pid1");
    let key = stack(&sb);
    // pid 1 leads its group and started long before this file: it passes the identity check, and is not ours.
    std::fs::write(key.pid_file(), "1\n").unwrap();
    let ran = run(&sb, &["--json", "down"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let v: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    assert_eq!(v["data"]["stopped"], false, "{}", ran.stdout);
    assert!(!key.pid_file().exists(), "the stale pid file was kept");
}

#[test]
fn leftovers_of_a_daemon_that_is_gone_are_removed_and_said() {
    let sb = fixture::Sandbox::new("down-gone");
    let key = stack(&sb);
    // A pid that cannot be alive: past any pid a system hands out.
    std::fs::write(key.pid_file(), "4294967295\n").unwrap();
    let ran = run(&sb, &["down"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert!(
        ran.stdout.contains("removed a leftover pid file"),
        "{}",
        ran.stdout
    );
    assert!(
        ran.stdout.ends_with("doze is not running\n"),
        "{}",
        ran.stdout
    );
    assert!(!key.pid_file().exists());
}

#[test]
fn a_daemon_that_does_not_stop_is_named_with_its_pid_after_twenty_seconds() {
    let sb = fixture::Sandbox::new("down-stuck");
    let key = stack(&sb);
    // A leader that ignores SIGTERM: what a daemon wedged in its shutdown looks like.
    let mut child = Command::new("sh")
        .args(["-c", "trap '' TERM; while :; do sleep 1; done"])
        .process_group(0)
        .spawn()
        .unwrap();
    let pid = child.id();
    std::fs::write(key.pid_file(), format!("{pid}\n")).unwrap();
    let started = std::time::Instant::now();
    let ran = run(&sb, &["--json", "down"]);
    let waited = started.elapsed();
    let survived = doze_os::process::alive(pid);
    let _ = doze_os::process::kill(pid);
    let _ = child.wait();
    assert_eq!(ran.code, 8, "{}", ran.stdout);
    assert!(survived, "the stuck stand-in was stopped some other way");
    assert!(waited >= std::time::Duration::from_secs(20), "{waited:?}");
    let v: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    assert_eq!(v["error"]["code"], "timeout", "{}", ran.stdout);
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains(&format!("(pid {pid}) did not stop")),
        "{}",
        ran.stdout
    );
    // Nothing is cleared while the daemon may still be there.
    assert!(key.pid_file().exists());
}

/// Go's `rememberDown`: what was awake is added to what `up` recorded, before the daemon is stopped.
#[test]
fn down_remembers_what_was_awake_for_the_next_up() {
    let sb = fixture::Sandbox::new("down-remember");
    let key = stack(&sb);
    std::fs::write(
        sb.project().join("doze.hcl"),
        "stack = \"shop\"\napp \"api\" { run = \"a\" }\napp \"worker\" { run = \"w\" }\napp \"web\" { run = \"x\" }\n",
    )
    .unwrap();
    doze_projects::update(key.dir(), |p| p.last_set = vec!["api".into()]).unwrap();
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/daemon/down-awake.ndjson");
    let script = doze_control::fixture::Script::load(&script).unwrap();
    let daemon = doze_control::fixture::FixtureDaemon::start(&key.socket(), script).unwrap();
    let (pid, reaper) = stand_in(&key, true);
    let ran = run(&sb, &["down"]);
    // `down` removed its socket, so the fixture could not wake its accept loop to shut it down: left to the process.
    std::mem::forget(daemon);
    if ran.code != 0 {
        let _ = doze_os::process::kill(pid);
    }
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    reaper.join().unwrap();
    let r = doze_projects::load(key.dir()).unwrap().unwrap();
    assert_eq!(r.last_set, ["api", "worker"]);
    assert!(r.last_down.is_some());
    assert_eq!(r.name, "shop");
}

#[test]
fn down_with_nothing_running_still_records_the_down() {
    let sb = fixture::Sandbox::new("down-record");
    let key = stack(&sb);
    let ran = run(&sb, &["down"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let r = doze_projects::load(key.dir()).unwrap().unwrap();
    assert!(r.last_down.is_some() && r.last_set.is_empty());
}
