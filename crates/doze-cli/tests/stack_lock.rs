//! The stack lock (D1's `run/doze.ctl.lock`): `down` takes it even on a stack that never ran (its run folder is made),
//! `up` takes it against a running daemon too (a `down` never stops a stack half-booted), and either waits, saying so,
//! while another holds it. And a daemon that answers a moment before its pid file is written is waited for, not
//! refused. The test holds the lock itself: the OS lock is per open file, so it excludes the in-process CLI too.
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
use doze_daemon::ctl::{RunDir, StackLock};
use doze_projects::StackKey;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::time::{Duration, Instant};

const HELD: Duration = Duration::from_millis(600);

fn key(sb: &fixture::Sandbox) -> StackKey {
    StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"))
}

/// Holds the stack's lock for [`HELD`] on a thread; returns once it is held.
fn hold(key: &StackKey) -> std::thread::JoinHandle<()> {
    let run = RunDir::of(key.dir());
    let (tx, rx) = std::sync::mpsc::channel();
    let t = std::thread::spawn(move || {
        let lock = StackLock::take(&run).unwrap();
        tx.send(()).unwrap();
        std::thread::sleep(HELD);
        drop(lock);
    });
    rx.recv().unwrap();
    t
}

#[test]
fn down_on_a_stack_that_never_ran_takes_the_lock_in_a_new_run_folder() {
    let sb = fixture::Sandbox::new("lock-fresh");
    std::fs::write(sb.project().join("doze.hcl"), "stack = \"shop\"\n").unwrap();
    let key = key(&sb);
    assert!(!key.run_dir().exists());
    let host = fixture::FakeHost::default();
    let ran = fixture::run(&sb, &host, Terminals::default(), false, &["down"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert!(
        ran.stdout.ends_with("doze is not running\n"),
        "{}",
        ran.stdout
    );
    assert!(key.run_dir().join("doze.ctl.lock").exists());
    // Held by another: down waits for it, and says so.
    let holder = hold(&key);
    let t = Instant::now();
    let ran = fixture::run(&sb, &host, Terminals::default(), false, &["down"]);
    holder.join().unwrap();
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert!(
        t.elapsed() >= HELD - Duration::from_millis(100),
        "{:?}",
        t.elapsed()
    );
    assert!(
        ran.stderr
            .contains("waiting for another doze up, run or down"),
        "{}",
        ran.stderr
    );
}

#[test]
fn up_against_a_running_daemon_waits_for_the_lock() {
    if !setup::sockets() {
        return;
    }
    let (sb, host, d) = setup::prepare(&["project shop".to_owned(), "daemon up".to_owned()]);
    let d = d.unwrap();
    let holder = hold(&key(&sb));
    let t = Instant::now();
    let ran = fixture::run(&sb, &host, Terminals::default(), false, &["up", "db"]);
    holder.join().unwrap();
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert!(
        t.elapsed() >= HELD - Duration::from_millis(100),
        "{:?}",
        t.elapsed()
    );
    assert!(
        ran.stderr
            .contains("waiting for another doze up, run or down"),
        "{}",
        ran.stderr
    );
    assert!(d.requests().iter().any(|r| r.op == "up" && r.db == "db"));
    // A dry run takes no lock.
    let holder = hold(&key(&sb));
    let plan = fixture::run(
        &sb,
        &host,
        Terminals::default(),
        false,
        &["up", "db", "--dry-run"],
    );
    assert_eq!(plan.code, 0, "{}", plan.stderr);
    assert!(!plan.stderr.contains("waiting"), "{}", plan.stderr);
    holder.join().unwrap();
}

/// D1 writes the pid file just after its socket accepts: a `down` in that instant waits for it and stops the daemon.
#[test]
fn down_waits_briefly_for_the_pid_file_of_a_daemon_that_just_started() {
    let sb = fixture::Sandbox::new("lock-pid");
    std::fs::write(sb.project().join("doze.hcl"), "stack = \"shop\"\n").unwrap();
    let key = key(&sb);
    std::fs::create_dir_all(key.run_dir()).unwrap();
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/daemon/down-awake.ndjson");
    let script = doze_control::fixture::Script::load(&script).unwrap();
    let daemon = doze_control::fixture::FixtureDaemon::start(&key.socket(), script).unwrap();
    let mut child: std::process::Child = std::process::Command::new("sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .unwrap();
    let pid = child.id();
    let reaper = std::thread::spawn(move || child.wait().unwrap()); // a zombie would still look alive
    let pid_file = key.pid_file();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        std::fs::write(pid_file, format!("{pid}\n")).unwrap();
    });
    let host = fixture::FakeHost::default();
    let ran = fixture::run(&sb, &host, Terminals::default(), false, &["--json", "down"]);
    std::mem::forget(daemon); // its socket is removed: left to the process, as tests/down.rs does
    writer.join().unwrap();
    if ran.code != 0 {
        let _ = doze_os::process::kill(pid);
    }
    let status = reaper.join().unwrap();
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert!(ran.stdout.contains("\"stopped\": true"), "{}", ran.stdout);
    assert_eq!(status.signal(), Some(15), "not ended by SIGTERM");
}
