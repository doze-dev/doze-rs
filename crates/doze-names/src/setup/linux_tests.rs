//! The privileged script, asserted as text. Built from facts, not from the machine, so these run on every system (Go's
//! were Linux-only and never ran on a Mac).

use super::super::{Daemon, Os, Paths};
use super::*;

const HOSTS: &str = "127.0.0.1\tlocalhost\n";

fn machine(root: &Path) -> Setup {
    Setup {
        os: Os::Linux,
        paths: Paths::under(root),
        home: PathBuf::from("/home/me/.doze"),
        daemon: Daemon {
            program: PathBuf::from("/usr/local/bin/doze"),
            args: vec![crate::HELPER_ARG.into()],
        },
        user: Some((1000, 1000)),
    }
}

fn facts(manager: Manager, dnsmasq: bool) -> Facts {
    Facts {
        manager,
        dnsmasq,
        hosts: Some(HOSTS.into()),
    }
}

fn script(s: &Setup, f: &Facts) -> String {
    let plan = install_plan(s, f).unwrap();
    assert!(
        !plan.script.is_empty(),
        "every assertion below would pass vacuously"
    );
    plan.script
}

use std::path::PathBuf;

/// Go: TestTheSysctlAppliesOnlyOurOwnFile. `sysctl --system` reapplies every sysctl.d file to set one key; in a
/// container it fails on read-only keys that have nothing to do with doze and `set -e` aborts before the hosts block.
#[test]
fn the_sysctl_applies_only_our_own_file() {
    let s = machine(Path::new("/r"));
    let text = script(&s, &facts(Manager::Resolved, false));
    assert!(!text.contains("--system"), "{text}");
    assert!(
        text.contains(&format!("sysctl -q -p {}", q(&s.paths.sysctl))),
        "{text}"
    );
}

/// Go: TestAFailedSysctlDoesNotAbortTheInstall. The key is the one optional step: its line tolerates failing, and the
/// hosts block it protects comes after it.
#[test]
fn a_failed_sysctl_does_not_abort_the_install() {
    let s = machine(Path::new("/r"));
    let text = script(&s, &facts(Manager::Plain, false));
    let line = text.lines().find(|l| l.starts_with("sysctl ")).unwrap();
    assert!(line.ends_with("|| true"), "{line}");
    let (sysctl, hosts) = (
        text.find("sysctl -q -p").unwrap(),
        text.find(&format!("> {}", q(&s.paths.hosts))).unwrap(),
    );
    assert!(
        sysctl < hosts,
        "the hosts block is written after the sysctl:\n{text}"
    );
}

/// Go: TestTheSysctlWriteSurvivesAMissingDirectory. debian:stable-slim ships no /etc/sysctl.d: the write makes it, and
/// is allowed to fail like the apply.
#[test]
fn the_sysctl_write_survives_a_missing_directory() {
    let s = machine(Path::new("/r"));
    let text = script(&s, &facts(Manager::Resolved, false));
    assert!(
        text.contains(&format!("mkdir -p {}", q(s.paths.sysctl.parent().unwrap()))),
        "{text}"
    );
    assert!(
        text.contains(&format!("> {} || true", q(&s.paths.sysctl))),
        "{text}"
    );
}

/// Go: TestTheLinuxDaemonUnitRunsAsThePersonAfterTheInterface. The daemon's unit runs the copy as the person who ran
/// setup, after the interface it binds exists, and carries this build's version so check can tell an older one.
#[test]
fn the_daemon_unit_runs_as_the_person_after_the_interface() {
    let s = machine(Path::new("/"));
    let u = netd_unit(&s, 1000, 1000);
    for want in [
        "User=1000\n".to_string(),
        "Group=1000\n".into(),
        format!("After={ZONE_UNIT_NAME}\n"),
        format!(
            "ExecStart=/usr/local/libexec/doze-netd {} /home/me/.doze/names.json\n",
            crate::HELPER_ARG
        ),
        "Restart=always\n".into(),
        format!("{}\n", netd_stamp()),
    ] {
        assert!(u.contains(&want), "the unit lacks {want:?}:\n{u}");
    }
    assert_eq!(netd_serves(&u), Some("/home/me/.doze/names.json"));
    let dir = std::env::temp_dir().join(format!("doze-names-unit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("doze-netd.service");
    assert!(
        !job_outdated(&path, &netd_stamp()),
        "no unit is not an old unit"
    );
    std::fs::write(&path, &u).unwrap();
    assert!(
        !job_outdated(&path, &netd_stamp()),
        "this build's unit is current"
    );
    std::fs::write(
        &path,
        u.replace(&netd_stamp(), "Environment=DOZE_NETD_VERSION=1"),
    )
    .unwrap();
    assert!(
        job_outdated(&path, &netd_stamp()),
        "a unit naming an earlier version is old"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Go: TestTheLinuxScriptInstallsTheDaemonWhereTheZoneIsRouted. Where the zone can be routed the script installs the
/// daemon beside the interface unit (resolved: no dnsmasq drop-in; dnsmasq: its drop-in too), where it cannot it
/// installs neither; the uninstall takes both away. Go checked whichever case the machine was; this checks all three.
#[test]
fn the_script_installs_the_daemon_where_the_zone_is_routed() {
    let s = machine(Path::new("/r"));
    for (f, routed) in [
        (facts(Manager::Resolved, false), true),
        (facts(Manager::Plain, true), true),
        (facts(Manager::NetworkManager, false), false),
    ] {
        let text = script(&s, &f);
        for want in [
            "cp -f ".to_string(),
            q(&s.paths.netd_program),
            format!("> {}", q(&s.paths.netd_unit)),
            format!("systemctl enable --now {NETD_UNIT_NAME}"),
        ] {
            assert_eq!(
                text.contains(&want),
                routed,
                "{:?}: {want:?} in\n{text}",
                f.manager
            );
        }
        let dnsmasq_drop = q(&s.paths.dnsmasq_dir.join("doze.conf"));
        assert_eq!(
            text.contains(&dnsmasq_drop),
            routed && f.manager != Manager::Resolved
        );
        assert_eq!(
            text.contains("resolvectl dns"),
            f.manager == Manager::Resolved
        );
    }
    let dir = std::env::temp_dir().join(format!("doze-names-un-{}", std::process::id()));
    let s = machine(&dir);
    std::fs::create_dir_all(dir.join("etc")).unwrap();
    std::fs::write(&s.paths.hosts, replace_block(HOSTS, &apex_block())).unwrap();
    let un = uninstall_plan(&s, &[]).unwrap().script;
    for want in [
        format!("systemctl disable --now {NETD_UNIT_NAME}"),
        q(&s.paths.netd_unit),
        q(&s.paths.netd_program),
        format!("printf '%s' {} > {}", shell_quote(HOSTS), q(&s.paths.hosts)),
    ] {
        assert!(un.contains(&want), "the uninstall lacks {want:?}:\n{un}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A hosts file with a `%` or a backslash in it is written as it was (Go's printf took the text as its format).
#[test]
fn the_hosts_text_is_never_read_as_a_format() {
    let s = machine(Path::new("/r"));
    let mut f = facts(Manager::Plain, false);
    f.hosts = Some("10.0.0.1 pc%d\\n.local\n".into());
    let text = script(&s, &f);
    assert!(
        text.contains("printf '%s' '10.0.0.1 pc%d\\n.local\n"),
        "{text}"
    );
}
