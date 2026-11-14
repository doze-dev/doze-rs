//! The Go CLI's golden files (doze `cmd/doze/testdata`, copied to `tests/fixtures/go` at doze 23b73d6), held byte for
//! byte against what this doze prints on stdout for the same scripted daemon (core plan C1's gate). The daemon plays
//! `tests/fixtures/daemon/*.ndjson`, which carry the Go goldens' snapshot (`shopSnapshot`) and frames.
//!
//! One difference is by design and is checked as such: Go's goldens capture stdout and stderr together, and a
//! failure here is a diagnostic on stderr (the `✗ events: …` line of `wake-failed` is `error[DZ0703]: events: …`).
//! `status-daemon-down` is read from the doze.hcl Go's golden test parses (`shopConfig`).
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
use std::path::Path;

fn golden(name: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/go/{name}.golden"));
    std::fs::read_to_string(p).unwrap()
}

/// Runs each command line against a fresh fixture daemon playing `script`, and returns stdout, all of it.
fn stdout_of(script: &str, commands: &[&str]) -> String {
    let lines: Vec<String> = [
        "project bare".to_owned(),
        format!("daemon {script}"),
        "now 2026-10-10T09:15:00Z".to_owned(),
        "env TZ=UTC".to_owned(),
    ]
    .into();
    let (sb, host, _daemon) = setup::prepare(&lines);
    let mut out = String::new();
    for c in commands {
        let args: Vec<&str> = c.split_whitespace().collect();
        let ran = fixture::run(&sb, &host, Terminals::default(), false, &args);
        assert!(
            ran.code == 0 || c.contains("wake db events api"),
            "{c}: exit {} {}",
            ran.code,
            ran.stderr
        );
        out.push_str(&ran.stdout);
    }
    out
}

#[test]
fn status_is_go_s_table() {
    if !setup::sockets() {
        return;
    }
    // Go's renderTable has no trailing newline after the summary line's own; fmt.Fprintln adds one each.
    assert_eq!(stdout_of("shop", &["status"]), golden("status"));
}

#[test]
fn status_watch_is_go_s_redraws() {
    if !setup::sockets() {
        return;
    }
    assert_eq!(stdout_of("watch", &["status -w"]), golden("status-watch"));
}

#[test]
fn wake_is_go_s() {
    if !setup::sockets() {
        return;
    }
    assert_eq!(stdout_of("wake", &["wake db events api"]), golden("wake"));
}

#[test]
fn a_failed_wake_keeps_go_s_lines_and_says_the_failure_on_stderr() {
    if !setup::sockets() {
        return;
    }
    let want: String = golden("wake-failed")
        .lines()
        .filter(|l| !l.starts_with('✗'))
        .map(|l| format!("{l}\n"))
        .collect();
    assert_eq!(stdout_of("wake-failed", &["wake db events api"]), want);
}

#[test]
fn sleep_restart_reset_pin_are_go_s() {
    if !setup::sockets() {
        return;
    }
    let got = stdout_of(
        "verbs",
        &[
            "sleep cache web",
            "sleep",
            "restart api",
            "reset -y db",
            "pin events cache",
        ],
    );
    assert_eq!(got, golden("sleep-restart-reset-pin"));
}

#[test]
fn logs_are_go_s() {
    if !setup::sockets() {
        return;
    }
    let cases = [
        ("logs-one", "logs api --run 2"),
        ("logs-previous-run", "logs api --run 1"),
        ("logs-merged", "logs api worker db --since 1s"),
        ("logs-follow", "logs -f api worker"),
        ("logs-nothing-yet", "logs web"),
    ];
    for (g, c) in cases {
        assert_eq!(stdout_of("logs", &[c]), golden(g), "{g}");
    }
}

/// With the daemon down, the services doze.hcl declares, from the same stack as Go's golden, byte for byte.
#[test]
fn status_with_the_daemon_down_lists_the_declared_services() {
    assert_eq!(stdout_of_down(), golden("status-daemon-down"));
}

fn stdout_of_down() -> String {
    let (sb, host, _d) = setup::prepare(&["project shop".to_owned()]);
    fixture::run(&sb, &host, Terminals::default(), false, &["status"]).stdout
}
