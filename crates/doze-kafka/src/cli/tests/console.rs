//! The console flags (parsed, checked and acted on by `web.rs`) and the start-up rule on their inputs.

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

fn with_sasl<'a>(extra: &[&'a str]) -> Vec<&'a str> {
    SASL.iter().chain(extra).copied().collect()
}

#[test]
fn the_console_flags_are_parsed_and_kept_and_their_help_says_what_they_do() {
    let set = settings(&[
        "--console-allow-host",
        "Dev.Example.com, host.docker.internal",
        "--console-allow-host",
        " ,*",
        "--console-unprotected",
    ])
    .unwrap();
    assert_eq!(
        set.console.allow_hosts,
        ["Dev.Example.com", "host.docker.internal", "*"]
    );
    assert!(set.console.unprotected);
    assert!(set.warnings.is_empty());
    let parsed = args(&["--console-allow-host", "a", "--console-unprotected"]).unwrap();
    let ignored: &[&str] = if cfg!(feature = "console") {
        &[]
    } else {
        &["--console-allow-host", "--console-unprotected"]
    };
    assert_eq!(parsed.ignored(), ignored);
    let help = <Args as clap::CommandFactory>::command()
        .render_long_help()
        .to_string();
    for flag in [
        "--console",
        "--console-addr",
        "--console-prefix",
        "--console-allow-host",
        "--console-unprotected",
    ] {
        assert!(help.contains(flag), "{flag} is hidden from --help");
    }
    let help = help.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(!help.contains("Phase 9"), "{help}");
    assert!(help.contains("127.0.0.1:9093"), "{help}");
    // SEC-241: the loopback console is open to every local process, and the help says how to turn it off.
    assert!(
        help.contains("on a shared machine any local process can use it without the broker's password or past its ACLs (it is a tool for one developer), so turn it off there with `--console=false`"),
        "{help}"
    );
    // SEC-250: ACLs made after the start close a console beyond loopback to other machines, unless --console-unprotected.
    assert!(
        help.contains("stops answering other machines while the broker has any ACL (made after the start, over Kafka or from the console), and answers them again once none is left"),
        "{help}"
    );
    assert!(settings(&[]).unwrap().console == Console::default());
}

#[test]
fn the_console_is_on_by_default_and_off_only_for_false_and_the_prefix_is_a_path() {
    let set = |flags: &[&str]| settings(flags).unwrap().console;
    assert!(set(&[]).enabled);
    assert!(set(&["--console"]).enabled);
    assert!(set(&["--console=true"]).enabled);
    assert!(!set(&["--console=false"]).enabled);
    assert_eq!(set(&[]).prefix, "/");
    assert_eq!(set(&["--console-prefix", "/kafka/"]).prefix, "/kafka/");
    for bad in ["/a b", "/a//b", "/../x", "/a/./b", "/é", "/a?b", "/a#b"] {
        let said = settings(&["--console-prefix", bad]).unwrap_err();
        assert!(said.contains("must be a path of letters"), "{bad}: {said}");
    }
    // A console that is off has no prefix to be wrong about.
    assert!(!set(&["--console=false", "--console-prefix", "/a b"]).enabled);
}

#[test]
fn a_console_address_that_is_not_host_and_port_is_refused() {
    for addr in [
        "9093",
        "host",
        "host:",
        "host:http",
        "host:99999",
        "a:b:9093",
        "127.0.0.1:-1",
    ] {
        let said = settings(&["--console-addr", addr]).unwrap_err();
        assert_eq!(
            said,
            format!(
                "config: console address {addr:?} is not host:port (for example 127.0.0.1:9093)"
            )
        );
    }
    // Turned off, the address is not looked at.
    assert!(settings(&["--console=false", "--console-addr", "nonsense"]).is_ok());
}

#[test]
fn sasl_without_tls_on_an_address_other_machines_reach_is_noted() {
    for (listen, noted) in [
        ("127.0.0.1:9092", false),
        ("[::1]:9092", false),
        ("0.0.0.0:9092", true),
        ("192.168.1.5:9092", true),
    ] {
        let set = settings(&with_sasl(&["--listen", listen])).unwrap();
        assert_eq!(
            set.warnings,
            if noted {
                vec![format!(
                    "SASL is on without TLS and --listen {listen} can be reached from other machines: logins (a PLAIN password is sent as it is) and messages cross the network unencrypted"
                )]
            } else {
                vec![]
            },
            "{listen}"
        );
    }
    // No SASL, nothing to note.
    assert!(
        settings(&["--listen", "0.0.0.0:9092"])
            .unwrap()
            .warnings
            .is_empty()
    );
}

#[test]
fn sasl_with_the_allow_default_is_noted_and_deny_or_no_sasl_is_not() {
    // SEC-191: the words are the audit's.
    let said = "SASL is on and --acl-default is allow, so every logged-in user may do everything, including managing users: use --acl-default deny with ACLs, or --acl-super-user";
    let open = settings(&["--sasl-mechanisms", "PLAIN", "--sasl-user", "u:pw-planted"]).unwrap();
    assert_eq!(open.warnings, [said]);
    let explicit = settings(&[
        "--sasl-mechanisms",
        "PLAIN",
        "--sasl-user",
        "u:pw-planted",
        "--acl-default",
        "allow",
        "--acl-super-user",
        "User:u",
    ])
    .unwrap();
    assert_eq!(explicit.warnings, [said]);
    // A warning, not an error: the broker starts. Deny, or no SASL at all, say nothing.
    assert!(settings(&with_sasl(&[])).unwrap().warnings.is_empty());
    assert!(
        settings(&["--acl-default", "allow"])
            .unwrap()
            .warnings
            .is_empty()
    );
    let help = <Args as clap::CommandFactory>::command()
        .render_long_help()
        .to_string();
    let help = help.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        help.contains("Only a super user may set or delete the SCRAM credential of a super user"),
        "{help}"
    );
}

#[test]
fn the_console_flag_takes_a_strict_boolean_in_any_case_and_anything_else_stops_the_start() {
    let set = |flags: &[&str]| settings(flags);
    for off in [
        "false", "0", "no", "off", "FALSE", "False", "No", "OFF", "oFf",
    ] {
        let flag = format!("--console={off}");
        assert!(!set(&[&flag]).unwrap().console.enabled, "{flag} left it on");
    }
    for on in ["true", "1", "yes", "on", "TRUE", "True", "Yes", "ON"] {
        let flag = format!("--console={on}");
        assert!(set(&[&flag]).unwrap().console.enabled, "{flag}");
    }
    // The value as a separate argument is the same, and the bare flag is on.
    assert!(!set(&["--console", "off"]).unwrap().console.enabled);
    assert!(set(&["--console"]).unwrap().console.enabled);
    for bad in [
        "maybe", "", "2", "fals", "truee", "t", "n", "disabled", " false",
    ] {
        let flag = format!("--console={bad}");
        let said = set(&[&flag]).unwrap_err();
        assert_eq!(
            said,
            format!(
                "config: --console takes true, false, 1, 0, yes, no, on or off, not {bad:?}; the console stays on only when it is asked for plainly"
            )
        );
    }
}

#[test]
fn a_console_flag_that_is_not_a_boolean_is_refused_even_when_the_console_flags_are_otherwise_fine()
{
    let said = settings(&["--console=maybe", "--console-addr", "127.0.0.1:0"]).unwrap_err();
    assert!(said.contains("--console takes true, false"), "{said}");
}
