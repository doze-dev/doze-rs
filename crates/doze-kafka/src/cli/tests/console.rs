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
fn sasl_and_a_console_beyond_loopback_is_refused_in_the_go_editions_words() {
    for addr in [
        "0.0.0.0:9093",
        ":9093",
        "192.168.1.5:9093",
        "[::]:9093",
        "kafka.lan:9093",
    ] {
        let said = settings(&with_sasl(&["--console-addr", addr])).unwrap_err();
        assert_eq!(
            said,
            format!(
                "config: the console has no login and is plain HTTP, and SASL is on: serving it on {addr} would let anyone who can reach that address act on the broker as a super user, without a password. Keep --console-addr on loopback (and put a proxy that authenticates in front of it), turn it off with --console=false, or accept this with --console-unprotected"
            )
        );
        assert!(!said.contains("planted"));
    }
}

#[test]
fn loopback_a_turned_off_console_and_no_protection_all_start() {
    for flags in [
        with_sasl(&["--console-addr", "127.0.0.1:9093"]),
        with_sasl(&["--console-addr", "[::1]:9093"]),
        with_sasl(&["--console-addr", "localhost:9093"]),
        with_sasl(&["--console-addr", "LOCALHOST:0"]),
        with_sasl(&["--console", "--console-addr", "127.0.0.1:0"]),
        with_sasl(&["--console=false", "--console-addr", "0.0.0.0:9093"]),
        // Nothing protects the broker, so there is nothing for the console to undo.
        vec!["--console-addr", "0.0.0.0:9093"],
    ] {
        let set = settings(&flags).unwrap_or_else(|e| panic!("{flags:?}: {e}"));
        assert!(
            set.warnings.iter().all(|w| !w.contains("console")),
            "{:?}",
            set.warnings
        );
    }
}

#[test]
fn unprotected_accepts_the_start_and_the_start_says_what_it_left_open() {
    let set = settings(&with_sasl(&[
        "--console-addr",
        "0.0.0.0:9093",
        "--console-unprotected",
    ]))
    .unwrap();
    assert_eq!(
        set.warnings,
        [
            "the console is served on 0.0.0.0:9093 with no login and over plain HTTP although SASL is on (--console-unprotected): that will let anyone who can reach that address act on the broker as a super user, without a password"
        ]
    );
    assert_eq!(set.console.addr.as_deref(), Some("0.0.0.0:9093"));
    // The flag with nothing to accept says nothing.
    let quiet = settings(&with_sasl(&[
        "--console-addr",
        "127.0.0.1:9093",
        "--console-unprotected",
    ]))
    .unwrap();
    assert!(quiet.warnings.is_empty());
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
