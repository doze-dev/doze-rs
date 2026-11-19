//! `doze down` on Windows: a daemon cannot be told from a stranger there yet (no process groups; the named-pipe daemon
//! is core plan W), so a pid file naming a live process is never signalled; it is a leftover, removed and said.
#![cfg(windows)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/fixture.rs"]
mod fixture;

use doze_cli::Terminals;

#[test]
fn a_live_pid_is_never_signalled_and_the_leftover_is_removed() {
    let sb = fixture::Sandbox::new("down-windows");
    std::fs::write(sb.project().join("doze.hcl"), "stack = \"shop\"\n").unwrap();
    let key = doze_projects::StackKey::for_config(&sb.home(), &sb.project().join("doze.hcl"));
    std::fs::create_dir_all(key.run_dir()).unwrap();
    // This test's own process: alive, and certainly not to be stopped.
    std::fs::write(key.pid_file(), format!("{}\n", std::process::id())).unwrap();
    let host = fixture::FakeHost::default();
    let ran = fixture::run(&sb, &host, Terminals::default(), false, &["--json", "down"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let v: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    assert_eq!(v["data"]["stopped"], false, "{}", ran.stdout);
    assert!(!key.pid_file().exists(), "the leftover pid file was kept");
}
