//! The privileged commands (`dns-setup`, `trust`, `doctor --fix`) end to end in-process, with the machine's files
//! under the sandbox and a host that records each script it is handed and runs none (the owner's Mac and the Lima VM
//! run them for real: scripts/core-privileged-e2e-darwin.sh, scripts/core-privileged-e2e-linux.sh).
//!
//! What is pinned: `--dry-run` shows the script and hands nothing over; a run hands over exactly one script (one
//! prompt), the system's prompt only when the app asked for it (`DOZE_ADMIN_PROMPT=1`); a cancelled prompt says the
//! words the app looks for; `doctor --fix` puts the zone and the stack's domains in the same script.
#![cfg(any(target_os = "macos", target_os = "linux"))] // WHY: the sh scripts; Windows' are doze-names' and CI's
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/fixture.rs"]
mod fixture;
#[path = "support/setup.rs"]
#[allow(dead_code)] // WHY: the transcripts' daemon helpers are not used here
mod setup;

use doze_cli::Terminals;
use doze_cli::ctx::Refused;
use fixture::{FakeHost, Ran, Sandbox};

const DOMAIN: &str = "dev.p1-cli-test.example";

/// A project with the zone on, the machine's files under the sandbox (a hosts file, as Linux setup reads one).
fn machine() -> (Sandbox, FakeHost) {
    let (sb, mut host, _) = setup::prepare(&["project pinned".to_owned()]);
    host.env.remove("DOZE_ZONE");
    let etc = sb.root.join("system/etc");
    std::fs::create_dir_all(&etc).unwrap();
    std::fs::write(etc.join("hosts"), "127.0.0.1 localhost\n").unwrap();
    (sb, host)
}

/// The domain recorded for the project, as its daemon does when the stack comes up.
fn record_domain(sb: &Sandbox) {
    let key = doze_doctor::domains::key(&sb.project());
    doze_names::routes::Routes::open(&sb.home())
        .record(&key, &[DOMAIN.to_owned()])
        .unwrap();
}

fn doze(sb: &Sandbox, host: &FakeHost, args: &[&str]) -> Ran {
    fixture::run(sb, host, Terminals::default(), false, args)
}

fn handed(host: &FakeHost) -> Vec<(String, String, bool)> {
    host.privileged.borrow().clone()
}

#[test]
fn dry_run_shows_the_script_and_hands_nothing_over() {
    let (sb, host) = machine();
    for args in [
        &["dns-setup", "--dry-run"][..],
        &["dns-setup", "--print"],
        &["doctor", "--print-fix"],
        &["doctor", "--fix", "--dry-run"],
    ] {
        let ran = doze(&sb, &host, args);
        assert_eq!(ran.code, 0, "{args:?}: {}", ran.stderr);
        assert!(ran.stdout.starts_with("# "), "{args:?}: {}", ran.stdout);
        assert!(
            ran.stdout.contains("sudo sh <<'DOZE_SETUP'\n"),
            "{}",
            ran.stdout
        );
        assert!(
            ran.stdout
                .contains(&sb.root.join("system").display().to_string())
        );
    }
    let ran = doze(&sb, &host, &["dns-setup", "--dry-run", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    assert_eq!(v["data"]["dry_run"], true);
    assert_eq!(v["data"]["applies_with"], "doze dns-setup");
    assert_eq!(v["data"]["shell"], "sh");
    assert!(v["data"]["script"].as_str().unwrap().contains("set -e"));
    assert!(handed(&host).is_empty());
}

/// One script, under sudo from a terminal; the setup is then checked, and with no real daemon behind the fake runner
/// it says the setup ran but is not in place (exit 10, the machine's problem), never "set up".
#[test]
fn a_run_hands_over_one_script_then_checks() {
    let (sb, host) = machine();
    let ran = doze(&sb, &host, &["dns-setup"]);
    let got = handed(&host);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(
        !got[0].2,
        "a terminal run asks sudo, not the system's prompt"
    );
    assert_eq!(ran.code, 10, "{}\n{}", ran.stdout, ran.stderr);
    assert!(ran.stderr.contains("setup ran, but"), "{}", ran.stderr);
    assert!(ran.stdout.contains("names: "), "{}", ran.stdout);
}

#[test]
fn the_app_gets_the_system_prompt_and_a_cancel_in_its_words() {
    let (sb, mut host) = machine();
    host.env.insert("DOZE_ADMIN_PROMPT".into(), "1".into());
    host.refuse = Some(Refused::Cancelled);
    let ran = doze(&sb, &host, &["dns-setup"]);
    let got = handed(&host);
    assert_eq!(got.len(), 1);
    assert!(got[0].2, "the app asked for the system's prompt");
    assert_eq!(ran.code, 1);
    // The app's doze_outcome looks for these words in what the command printed.
    assert!(
        ran.stderr
            .contains("the administrator prompt was cancelled"),
        "{}",
        ran.stderr
    );
    host.refuse = Some(Refused::Failed("launchctl: no such file".into()));
    let ran = doze(&sb, &host, &["dns-setup"]);
    assert_eq!(ran.code, 10);
    assert!(
        ran.stderr.contains("launchctl: no such file"),
        "{}",
        ran.stderr
    );
}

#[test]
fn check_reports_each_part_and_the_fix() {
    let (sb, host) = machine();
    let ran = doze(&sb, &host, &["dns-setup", "--check"]);
    assert_eq!(ran.code, 1, "{}", ran.stdout);
    assert!(ran.stdout.contains("names: "), "{}", ran.stdout);
    assert!(
        ran.stderr.contains("doze dns-setup (one sudo)"),
        "{}",
        ran.stderr
    );
    let ran = doze(&sb, &host, &["doctor", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    let names: Vec<&serde_json::Value> = v["data"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["name"].as_str().unwrap().starts_with("names: "))
        .collect();
    assert!(!names.is_empty());
    // The machine contract the app reads: `ok` on every check, the fix on a failing one.
    assert!(names.iter().all(|c| c["ok"].is_boolean()));
    assert!(
        names
            .iter()
            .filter(|c| c["ok"] == false)
            .all(|c| c["fix"] == "doze dns-setup")
    );
    assert!(handed(&host).is_empty());
}

/// `doctor --fix` with the zone missing and a recorded domain: one script for both, one prompt.
#[test]
fn the_repair_is_one_prompt_for_the_zone_and_the_domains() {
    let (sb, host) = machine();
    record_domain(&sb);
    let ran = doze(&sb, &host, &["doctor", "--fix", "--dry-run"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let first = ran.stdout.lines().next().unwrap();
    assert!(first.contains("; and "), "{first}");
    assert!(first.contains(DOMAIN), "{first}");
    let _ = doze(&sb, &host, &["doctor", "--fix"]);
    let got = handed(&host);
    assert_eq!(got.len(), 1, "one prompt: {got:?}");
    assert!(got[0].1.contains(DOMAIN));
    let ran = doze(&sb, &host, &["doctor"]);
    assert!(
        ran.stdout.contains(&format!("domains: route {DOMAIN}")),
        "{}",
        ran.stdout
    );
}

#[test]
fn trust_routes_the_recorded_domains() {
    let (sb, host) = machine();
    record_domain(&sb);
    let ran = doze(&sb, &host, &["trust", "--check"]);
    assert_eq!(ran.code, 1, "{}", ran.stdout);
    assert!(
        ran.stdout.contains(&format!("route {DOMAIN}")),
        "{}",
        ran.stdout
    );
    assert!(
        ran.stderr.contains("doze trust (one sudo)"),
        "{}",
        ran.stderr
    );

    let ran = doze(&sb, &host, &["trust", "--dry-run"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert!(ran.stdout.contains(DOMAIN), "{}", ran.stdout);
    assert!(handed(&host).is_empty());

    // Nothing really ran behind the recorder, so the route is still missing: said, exit 10.
    let ran = doze(&sb, &host, &["trust"]);
    assert_eq!(ran.code, 10, "{}", ran.stderr);
    assert!(
        ran.stderr.contains("is still not in place"),
        "{}",
        ran.stderr
    );
    assert_eq!(handed(&host).len(), 1);
    assert!(
        ran.stdout
            .contains(&format!("✓ https://{DOMAIN}  once the stack is up")),
        "{}",
        ran.stdout
    );

    let ran = doze(&sb, &host, &["trust", "--remove", "--dry-run"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert!(ran.stdout.contains(DOMAIN), "{}", ran.stdout);
    assert_eq!(handed(&host).len(), 1, "a dry run hands nothing over");
}

#[test]
fn uninstall_shows_and_runs_one_script() {
    let (sb, host) = machine();
    let ran = doze(&sb, &host, &["dns-setup", "--uninstall", "--dry-run"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert!(ran.stdout.starts_with("# "), "{}", ran.stdout);
    let ran = doze(&sb, &host, &["dns-setup", "--uninstall"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(handed(&host).len(), 1);
    assert!(ran.stdout.contains("✓ removed"));
}

/// The network daemon dns-setup installs is this program, and a word the app's argv dispatch hands to the CLI.
#[test]
fn the_daemon_word_is_a_command() {
    assert!(doze_cli::is_command(doze_names::HELPER_ARG));
}
