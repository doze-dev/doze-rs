//! What the verbs ask the daemon, checked on the requests it received (not only on what was printed): `wake` starts
//! what a service needs first; `pin` sends its toggle only where the state differs, and a name given twice once;
//! Ctrl-C during a verb ends the run at once with exit 130 and its envelope, as Go's does.
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
use std::time::{Duration, Instant};

fn setup(
    script: &str,
) -> (
    fixture::Sandbox,
    fixture::FakeHost,
    doze_control::fixture::FixtureDaemon,
) {
    let lines = vec!["project bare".to_owned(), format!("daemon {script}")];
    let (sb, host, d) = setup::prepare(&lines);
    (sb, host, d.unwrap())
}

/// The verbs the daemon was asked for, as `op target`.
fn verbs(d: &doze_control::fixture::FixtureDaemon) -> Vec<String> {
    d.requests()
        .into_iter()
        .filter(|r| !matches!(r.op.as_str(), "" | "status" | "subscribe" | "cancel"))
        .map(|r| format!("{} {}", r.op, r.db).trim().to_owned())
        .collect()
}

#[test]
fn wake_starts_what_a_service_needs_first() {
    if !setup::sockets() {
        return;
    }
    let (sb, host, d) = setup("shop");
    let ran = fixture::run(&sb, &host, Terminals::default(), false, &["wake", "worker"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(verbs(&d), ["wake events", "wake worker"]);
    assert_eq!(ran.stdout, "✓ events awake\n✓ worker awake\n");
}

#[test]
fn pin_toggles_only_what_differs_and_each_name_once() {
    if !setup::sockets() {
        return;
    }
    let (sb, host, d) = setup("shop"); // events is pinned, cache is not
    let ran = fixture::run(
        &sb,
        &host,
        Terminals::default(),
        false,
        &["pin", "events", "cache", "cache"],
    );
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(verbs(&d), ["pin cache"]);
    assert_eq!(ran.stdout.lines().count(), 2, "{}", ran.stdout);
    let again = fixture::run(
        &sb,
        &host,
        Terminals::default(),
        false,
        &["pin", "events", "cache"],
    );
    assert!(
        again.stdout.contains("cache already pinned"),
        "{}",
        again.stdout
    );
    assert_eq!(verbs(&d), ["pin cache"], "a second run sends nothing");
}

#[test]
fn ctrl_c_during_a_verb_exits_130_at_once() {
    if !setup::sockets() {
        return;
    }
    let (sb, host, _d) = setup("slow");
    let press = host.interrupt.clone();
    let t = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        press.raise();
    });
    let start = Instant::now();
    let ran = fixture::run(
        &sb,
        &host,
        Terminals::default(),
        false,
        &["--json", "restart", "api", "db"],
    );
    t.join().unwrap();
    assert!(
        start.elapsed() < Duration::from_secs(3),
        "it waited for the verb"
    );
    assert_eq!(ran.code, 130, "{}", ran.stdout);
    let v: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    assert_eq!(v["error"]["code"], "interrupted");
    assert_eq!(v["data"]["results"][0]["error"], "interrupted");
}

/// A follow on a stream that never ends stops on Ctrl-C the way a follow is meant to stop: its lines, the end line,
/// exit 0.
#[test]
fn ctrl_c_ends_a_follow_cleanly() {
    if !setup::sockets() {
        return;
    }
    let (sb, host, _d) = setup("held");
    let press = host.interrupt.clone();
    let t = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        press.raise();
    });
    let ran = fixture::run(
        &sb,
        &host,
        Terminals::default(),
        false,
        &["--json", "logs", "-f", "api"],
    );
    t.join().unwrap();
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let lines: Vec<&str> = ran.stdout.lines().collect();
    assert_eq!(lines.len(), 2, "{}", ran.stdout);
    assert!(lines[0].starts_with("{\"type\":\"log\""));
    assert_eq!(lines[1], "{\"type\":\"end\",\"ok\":true}");
}
