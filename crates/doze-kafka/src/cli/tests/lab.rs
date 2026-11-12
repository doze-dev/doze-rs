//! The Lab flag and its start-up rule (LAB-30): on by default for a dev tool, off with `--lab=false`, off with the console, and
//! off by default where the console is open to other machines in front of a broker that is protected.

use super::*;

const SASL: [&str; 6] = [
    "--sasl-mechanisms",
    "PLAIN",
    "--sasl-user",
    "u:pw-planted",
    "--acl-default",
    "deny",
];

fn settings(flags: &[&str]) -> Result<crate::cli::Settings, String> {
    args(flags)
        .unwrap()
        .settings()
        .map_err(|e| format!("{e:#}"))
}

fn protected<'a>(extra: &[&'a str]) -> Vec<&'a str> {
    SASL.iter().chain(extra).copied().collect()
}

#[test]
fn the_lab_is_on_by_default_and_off_with_its_flag_or_with_the_console() {
    assert!(settings(&[]).unwrap().console.lab);
    assert!(settings(&["--lab"]).unwrap().console.lab);
    for off in ["false", "0", "no", "off", "FALSE"] {
        assert!(
            !settings(&[&format!("--lab={off}")]).unwrap().console.lab,
            "{off}"
        );
    }
    assert!(!settings(&["--console=false"]).unwrap().console.lab);
    assert!(
        !settings(&["--console=false", "--lab=true"])
            .unwrap()
            .console
            .lab
    );
    let said = settings(&["--lab=maybe"]).unwrap_err();
    assert!(said.contains("--lab takes true, false"), "{said}");
}

#[test]
fn a_console_open_to_other_machines_says_the_lab_can_inject_faults() {
    let set = settings(&["--console-addr", "0.0.0.0:9093"]).unwrap();
    assert!(set.console.lab);
    assert_eq!(set.warnings.len(), 1, "{:?}", set.warnings);
    assert!(set.warnings[0].contains("anyone who can reach the console can inject faults"));
    let off = settings(&["--console-addr", "0.0.0.0:9093", "--lab=false"]).unwrap();
    assert!(off.warnings.is_empty());
}

#[test]
fn a_protected_broker_with_an_open_console_has_the_lab_off_unless_asked_and_never_without_unprotected()
 {
    let refused = settings(&protected(&[
        "--console-addr",
        "0.0.0.0:9093",
        "--lab=true",
    ]))
    .unwrap_err();
    assert!(refused.contains("--console-unprotected"), "{refused}");
    let set = settings(&protected(&[
        "--console-addr",
        "0.0.0.0:9093",
        "--console-unprotected",
    ]))
    .unwrap();
    assert!(!set.console.lab);
    assert!(
        set.warnings
            .iter()
            .any(|w| w.contains("the Lab is off because"))
    );
    let on = settings(&protected(&[
        "--console-addr",
        "0.0.0.0:9093",
        "--console-unprotected",
        "--lab=true",
    ]))
    .unwrap();
    assert!(on.console.lab);
    assert!(on.warnings.iter().any(|w| w.contains("inject faults")));
    // On loopback a protected broker keeps the Lab: only this machine reaches the console.
    assert!(settings(&protected(&[])).unwrap().console.lab);
}
