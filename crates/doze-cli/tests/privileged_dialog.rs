//! The privileged commands when macOS's own trust dialog is part of the plan (the person's parts, around root's), in
//! process as in `privileged.rs`: a cancelled dialog is said in the words the app reads, a removal's dialog comes
//! before root's part so a cancel keeps everything, and `--json` carries every part.
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

/// macOS's own dialog closed: the CLI says so in the words the app reads as that cancel (not its own prompt's, whose
/// "nothing was changed" would be false once root's part ran). A removal's dialog comes before root's part, so a
/// cancelled `trust --remove` or `dns-setup --uninstall` keeps the authority's files and the record as they were.
#[test]
fn a_cancelled_macos_dialog_is_said_and_a_cancelled_removal_keeps_everything() {
    use doze_names::setup::{MAC_DIALOG_CANCELLED, UserPart, UserRefused};
    let (sb, mut host) = machine();
    host.refuse = Some(Refused::Dialog {
        why: UserRefused::Cancelled(UserPart::Trust).to_string(),
        after_root: true,
    });
    let ran = doze(&sb, &host, &["dns-setup"]);
    assert_eq!(ran.code, 1, "{}", ran.stderr);
    assert!(
        ran.stderr.contains(MAC_DIALOG_CANCELLED)
            && ran.stderr.contains("not trusted yet")
            && !ran
                .stderr
                .contains("the administrator prompt was cancelled"),
        "{}",
        ran.stderr
    );

    host.env.insert("DOZE_ZONE".into(), "off".into());
    record_domain(&sb);
    let root = doze_names::ca::ca_dir(&sb.home()).join("root.crt");
    doze_names::ca::Ca::load_or_create(&doze_names::ca::ca_dir(&sb.home())).unwrap();
    host.refuse = Some(Refused::Dialog {
        why: UserRefused::Cancelled(UserPart::Untrust).to_string(),
        after_root: false,
    });
    for args in [
        &["trust", "--remove"][..],
        &["dns-setup", "--uninstall"][..],
    ] {
        let ran = doze(&sb, &host, args);
        assert_eq!(ran.code, 1, "{args:?}: {}", ran.stderr);
        assert!(ran.stderr.contains("still trusted"), "{}", ran.stderr);
        assert!(root.is_file(), "{args:?}: the authority's files stay");
        let stack = doze_doctor::domains::StackDomains::recorded(&sb.home(), &sb.project());
        assert_eq!(stack.domains, [DOMAIN], "{args:?}: the record stays");
    }
}

/// `--json` carries each part of a dry run: a removal's part for the person, which runs before root's (macOS), is
/// there as `as_user_before`, and absent where there is none.
#[test]
fn a_dry_runs_json_carries_the_part_before_roots() {
    let (sb, mut host) = machine();
    host.env.insert("DOZE_ZONE".into(), "off".into());
    record_domain(&sb);
    doze_names::ca::Ca::load_or_create(&doze_names::ca::ca_dir(&sb.home())).unwrap();
    let ran = doze(&sb, &host, &["trust", "--remove", "--dry-run", "--json"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let v: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    let before = &v["data"]["as_user_before"];
    if cfg!(target_os = "macos") {
        assert!(
            before
                .as_str()
                .is_some_and(|s| s.contains("remove-trusted-cert -d")),
            "{}",
            ran.stdout
        );
    } else {
        assert!(before.is_null(), "{}", ran.stdout);
    }
    assert!(v["data"]["as_user"].is_null(), "{}", ran.stdout);
}
