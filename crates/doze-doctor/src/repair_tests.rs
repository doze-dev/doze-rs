use super::*;
use crate::domains::StackDomains;
use crate::{Check, State, leftovers};
use doze_names::setup::{Daemon, Os, Paths, Setup};
use std::path::Path;

fn sh(what: &str, script: &str) -> Plan {
    Plan {
        what: what.into(),
        shell: Shell::Sh,
        script: script.into(),
    }
}

/// A Mac set up nowhere: every system file under a temporary root, the person a fixed uid.
fn machine(root: &Path) -> Machine {
    Machine {
        setup: Setup {
            os: Os::Mac,
            paths: Paths::under(root),
            home: root.join("home"),
            daemon: Daemon {
                program: "/opt/doze/bin/doze".into(),
                args: vec![doze_names::HELPER_ARG.into()],
            },
            user: Some((501, 20)),
        },
        zone_off: false,
    }
}

#[test]
fn one_plan_is_itself_and_none_is_nothing() {
    let p = sh("route .doze", "echo a\n");
    assert_eq!(combine([&p]), Some(p.clone()));
    assert_eq!(combine(std::iter::empty::<&Plan>()), None);
}

/// The combined script runs each part in order, keeps each part's heredocs and `set -e` to itself, and stops at the
/// first part that fails (the second part's words never print).
#[cfg(unix)] // WHY: it asks /bin/sh to run the script
#[test]
fn parts_run_in_order_and_a_failure_stops_the_rest() {
    let a = sh(
        "write a file",
        "set -e\ncat <<'X'\nfirst 'quoted'\nX\nV=1\n",
    );
    let b = sh("say the second", "echo \"second ${V:-unset}\"\n");
    let both = combine([&a, &b]).unwrap();
    assert_eq!(both.what, "write a file; and say the second");
    let out = std::process::Command::new("/bin/sh")
        .args(["-c", &both.script])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "first 'quoted'\nsecond unset\n"
    );
    let failing = sh("fail", "false\necho never\n");
    let stop = combine([&failing, &b]).unwrap();
    let out = std::process::Command::new("/bin/sh")
        .args(["-c", &stop.script])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

/// A machine with nothing set up: the names checks fail with `doze dns-setup` as the fix (https with `doze doctor
/// --fix`), and the repair is the zone's install and the authority's trust under one prompt: a first-time person types
/// one password and gets https:// for every `.doze` name, with no domain declared.
#[test]
fn nothing_set_up_is_repaired_by_the_zone_install() {
    let dir = tempfile::tempdir().unwrap();
    let m = machine(dir.path());
    let checks = names::checks(&m);
    assert!(!checks.is_empty());
    let failing: Vec<&Check> = checks.iter().filter(|c| c.state == State::Fail).collect();
    assert!(!failing.is_empty(), "{checks:?}");
    assert!(failing.iter().all(|c| c.name.starts_with("names: ")
        && c.fix.as_deref()
            == Some(if c.name == crate::https::NAME {
                crate::FIX_HTTPS
            } else {
                crate::FIX_NAMES
            })
        && !c.ok));
    let https = failing
        .iter()
        .find(|c| c.name == crate::https::NAME)
        .unwrap();
    assert!(
        https
            .detail
            .contains("https:// names are refused until doze doctor --fix"),
        "{}",
        https.detail
    );
    let r = plan(&m, None);
    let kinds: Vec<Kind> = r.parts.iter().map(|p| p.kind).collect();
    assert_eq!(kinds, [Kind::Names, Kind::Https]);
    let one = r.one().unwrap();
    assert!(one.script.contains("dev.doze.netd"), "{}", one.script);
    assert!(
        one.script
            .contains(&dir.path().join("etc/resolver").display().to_string())
    );
    let root = m.home().join("ca").join("root.crt").display().to_string();
    assert!(
        one.script.contains(&format!("add-trusted-cert -d -r trustRoot -p ssl -p basic -k /Library/Keychains/System.keychain '{root}'")),
        "{}",
        one.script
    );
    assert!(
        !m.home().join("ca").exists(),
        "planning (a dry run) makes nothing"
    );
    r.prepare().unwrap();
    assert!(
        m.home().join("ca").join("root.crt").is_file(),
        "the run makes the root the script trusts"
    );
}

/// A broken authority serves no name: with no domain declared the repair still says it cannot fix it and how, rather
/// than "nothing to do", and plans no trust of it; doctor's https line names the rotation.
#[test]
fn a_broken_authority_without_domains_is_said_not_passed_over() {
    let dir = tempfile::tempdir().unwrap();
    let m = machine(dir.path());
    let ca = m.home().join("ca");
    std::fs::create_dir_all(&ca).unwrap();
    std::fs::write(ca.join("root.crt"), "not a certificate").unwrap();
    std::fs::write(ca.join("root.key"), "not a key").unwrap();
    let r = plan(&m, None);
    assert!(
        r.cannot
            .iter()
            .any(|c| c.starts_with("the certificate authority")
                && c.ends_with("(doze trust --rotate)")),
        "{:?}",
        r.cannot
    );
    assert!(r.parts.iter().all(|p| p.kind != Kind::Https));
    let https = crate::https::check(&m).unwrap();
    assert_eq!(https.fix.as_deref(), Some(crate::FIX_ROTATE));
}

#[test]
fn a_zone_switched_off_needs_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = machine(dir.path());
    m.zone_off = true;
    let checks = names::checks(&m);
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].state, State::Ok);
    assert!(plan(&m, None).is_empty());
}

/// A domain the stack's daemon recorded is the stack's: it is checked, and the repair routes it in the same prompt as
/// the zone's setup.
#[test]
fn a_recorded_domain_is_checked_and_routed_with_the_zone() {
    let dir = tempfile::tempdir().unwrap();
    let m = machine(dir.path());
    let project = dir.path().join("shop");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("doze.hcl"), "").unwrap();
    let key = crate::domains::key(&project);
    let domain = "dev.p1-doctor-test.example";
    Routes::open(m.home())
        .record(&key, &[domain.to_owned()])
        .unwrap();
    let stack = StackDomains::recorded(m.home(), &project);
    assert_eq!(stack.domains, [domain]);
    let other = StackDomains::recorded(m.home(), dir.path());
    assert!(other.domains.is_empty());

    let checks = crate::domains::checks(&m, Some(&stack));
    let route = checks
        .iter()
        .find(|c| c.name == format!("domains: route {domain}"))
        .unwrap();
    assert_eq!(route.state, State::Fail);
    assert_eq!(route.fix.as_deref(), Some(crate::FIX_DOMAINS));
    assert!(
        checks
            .iter()
            .any(|c| c.name == "domains: certificate authority" && c.state == State::NotChecked)
    );

    let r = plan(&m, Some(&stack));
    let kinds: Vec<Kind> = r.parts.iter().map(|p| p.kind).collect();
    assert_eq!(kinds, [Kind::Names, Kind::Domains, Kind::Https]);
    let one = r.one().unwrap();
    assert!(one.what.contains(domain), "{}", one.what);
    assert!(one.script.contains(domain));
    assert!(one.script.starts_with("set -e\n(\n"));
    // macOS reads neither route until mDNSResponder is told to: each part that writes one tells it, after the write,
    // inside the one script (no second prompt).
    const HUP: &str = "killall -HUP mDNSResponder 2>/dev/null || true";
    // Built the way the plan builds it (the resolver folder, then the zone's file), so it matches on Windows too.
    let zone = format!(
        "> '{}'",
        dir.path().join("etc/resolver").join("doze").display()
    );
    let route = format!("> '/etc/resolver/{domain}'");
    let at = |needle: &str| {
        one.script
            .find(needle)
            .unwrap_or_else(|| panic!("no {needle:?} in\n{}", one.script))
    };
    let hups: Vec<usize> = one.script.match_indices(HUP).map(|(i, _)| i).collect();
    assert_eq!(hups.len(), 2, "{}", one.script);
    assert!(
        at(&zone) < hups[0] && hups[0] < at(&route) && at(&route) < hups[1],
        "{}",
        one.script
    );
}

#[test]
fn dead_names_are_leftovers() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    std::fs::write(
        home.join(doze_names::registry::FILE_NAME),
        r#"{"db.shop.doze":{"ip":"127.0.0.1","pid":2147483000,"owner":"doze"},
            "api.shop.doze":{"ip":"127.0.0.2","pid":0,"owner":"doze"}}"#,
    )
    .unwrap();
    assert_eq!(
        leftovers::dead_names(home),
        ["api.shop.doze", "db.shop.doze"]
    );
    let c = leftovers::checks(home);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].state, State::Warn);
    assert!(c[0].ok);
}

#[test]
fn a_check_says_ok_unless_it_fails_and_keeps_a_fix_only_when_failing() {
    let ok = Check::step("x", true, "fine").fix("doze trust");
    assert!(ok.ok && ok.fix.is_none());
    let bad = Check::step("x", false, "broken").fix("doze trust");
    assert!(!bad.ok && bad.fix.as_deref() == Some("doze trust"));
    let unknown = Check::new("x", State::NotChecked, "?");
    assert!(!unknown.ok, "not looked at is never a tick");
    assert_eq!(
        serde_json::to_value(&bad).unwrap(),
        serde_json::json!({"name":"x","state":"fail","ok":false,"detail":"broken","fix":"doze trust"})
    );
    assert!(
        crate::zone_off(Some(" OFF ")) && !crate::zone_off(Some("on")) && !crate::zone_off(None)
    );
}

/// PowerShell parts each run in their own script block under one stop-on-error preference; plans of two shells are
/// never combined.
#[test]
fn powershell_parts_are_blocks_and_shells_never_mix() {
    let ps = |what: &str, s: &str| Plan {
        what: what.into(),
        shell: Shell::PowerShell,
        script: s.into(),
    };
    let (a, b) = (ps("one", "Write-Output 1\n"), ps("two", "Write-Output 2\n"));
    let both = combine([&a, &b]).unwrap();
    assert_eq!(
        both.script,
        "$ErrorActionPreference = 'Stop'\n& {\nWrite-Output 1\n}\n& {\nWrite-Output 2\n}\n"
    );
    assert_eq!(combine([&a, &sh("x", "true\n")]), None);
}
