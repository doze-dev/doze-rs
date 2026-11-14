//! `doze down` stops the daemon its pid file names (SIGTERM to that exact pid), waits for it to go, and leaves no
//! socket or pid file behind (proposal rule 8). A stand-in process plays the daemon: the test's own child.
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

fn stack(sb: &fixture::Sandbox) -> doze_projects::StackKey {
    std::fs::write(sb.project().join("doze.hcl"), "stack = \"shop\"\n").unwrap();
    let key = doze_projects::StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"));
    std::fs::create_dir_all(key.run_dir()).unwrap();
    key
}

#[test]
fn down_stops_the_daemon_by_its_pid_and_leaves_nothing() {
    let sb = fixture::Sandbox::new("down");
    let key = stack(&sb);
    let mut daemon = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .unwrap();
    std::fs::write(key.pid_file(), format!("{}\n", daemon.id())).unwrap();
    // Reaped as it ends, as init reaps the real (detached) daemon: a zombie would still answer kill(pid, 0).
    let reaper = std::thread::spawn(move || daemon.wait().unwrap());
    // A socket file nobody listens on: what a daemon leaves when it is killed.
    std::fs::write(key.socket(), "").unwrap();
    let host = fixture::FakeHost::default();
    let ran = fixture::run(&sb, &host, Terminals::default(), false, &["--json", "down"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let v: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    assert_eq!(v["data"]["stopped"], true, "{}", ran.stdout);
    // The child was ended by SIGTERM.
    let status = reaper.join().unwrap();
    assert!(!status.success());
    assert!(!key.pid_file().exists());
    assert!(!key.socket().exists());
}

#[test]
fn leftovers_of_a_daemon_that_is_gone_are_removed_and_said() {
    let sb = fixture::Sandbox::new("down-gone");
    let key = stack(&sb);
    // A pid that cannot be alive: past any pid a system hands out.
    std::fs::write(key.pid_file(), "4294967295\n").unwrap();
    let host = fixture::FakeHost::default();
    let ran = fixture::run(&sb, &host, Terminals::default(), false, &["down"]);
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
