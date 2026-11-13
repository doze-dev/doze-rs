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

/// The refusal for a console on `addr`, in the words the start says, for `what` guards the broker and `exposure` it would undo.
fn refusal(what: &str, addr: &str, exposure: &str) -> String {
    format!(
        "config: the console has no login and is plain HTTP, and {what}: serving it on {addr} would let anyone who can reach that address {exposure}. Keep --console-addr on loopback (and put a proxy that authenticates in front of it), turn it off with --console=false, or accept this with --console-unprotected"
    )
}

const BEYOND: [&str; 5] = [
    "0.0.0.0:9093",
    ":9093",
    "192.168.1.5:9093",
    "[::]:9093",
    "kafka.lan:9093",
];

#[test]
fn each_guard_of_the_broker_alone_refuses_a_console_beyond_loopback_and_says_what_it_would_expose()
{
    // SEC-243: the console is never a wider door than the broker. Each guard alone, with Kafka itself on every interface.
    let open = ["--listen", "0.0.0.0:9092"];
    let sasl_alone = ["--sasl-mechanisms", "PLAIN", "--sasl-user", "u:pw-planted"];
    for addr in BEYOND {
        let flags: Vec<&str> = open
            .iter()
            .chain(&sasl_alone)
            .chain(&["--console-addr", addr])
            .copied()
            .collect();
        let said = settings(&flags).unwrap_err();
        assert_eq!(
            said,
            refusal(
                "SASL is on",
                addr,
                "act on the broker as a super user without a password"
            )
        );
        assert!(!said.contains("planted"));
        let said = settings(&[
            "--listen",
            "0.0.0.0:9092",
            "--acl-default",
            "deny",
            "--console-addr",
            addr,
        ])
        .unwrap_err();
        assert_eq!(
            said,
            refusal(
                "ACLs are in force (--acl-default deny)",
                addr,
                "do what the ACLs refuse (the console acts as a super user)"
            )
        );
        // Kafka on this machine only, the console on the network: a plain open broker on loopback.
        let said = settings(&["--console-addr", addr]).unwrap_err();
        assert_eq!(
            said,
            refusal(
                "--listen 127.0.0.1:9092 keeps Kafka on this machine",
                addr,
                "reach from the network a broker that Kafka clients can reach only from this machine"
            )
        );
        let said = settings(&["--listen", "[::1]:9092", "--console-addr", addr]).unwrap_err();
        assert!(
            said.contains("--listen [::1]:9092 keeps Kafka on this machine"),
            "{said}"
        );
        // TLS alone (a build without the feature refuses its flags, so the rule is asked directly; tests/tls.rs starts one).
        let tls = crate::cli::console::setup(&crate::cli::console::Input {
            tls: true,
            ..input(addr, "0.0.0.0:9092")
        })
        .unwrap_err();
        assert_eq!(
            format!("{tls:#}"),
            refusal(
                "TLS is on",
                addr,
                "read topics and messages sent unencrypted"
            )
        );
    }
}

/// The rule's input for a console on `addr` in front of a broker on `listen` with nothing else on.
fn input<'a>(addr: &'a str, listen: &str) -> crate::cli::console::Input<'a> {
    crate::cli::console::Input {
        console: None,
        addr: Some(addr),
        prefix: "/",
        allow_hosts: &[],
        unprotected: false,
        lab: None,
        sasl: false,
        tls: false,
        listen: listen.parse().unwrap(),
        acl_allow: true,
    }
}

#[test]
fn every_guard_at_once_is_named_in_one_refusal() {
    let said = settings(&with_sasl(&["--console-addr", "0.0.0.0:9093"])).unwrap_err();
    assert_eq!(
        said,
        refusal(
            "SASL is on, ACLs are in force (--acl-default deny) and --listen 127.0.0.1:9092 keeps Kafka on this machine",
            "0.0.0.0:9093",
            "act on the broker as a super user without a password, do what the ACLs refuse (the console acts as a super user) and reach from the network a broker that Kafka clients can reach only from this machine"
        )
    );
    let all = crate::cli::console::setup(&crate::cli::console::Input {
        sasl: true,
        tls: true,
        acl_allow: false,
        ..input("0.0.0.0:9093", "127.0.0.1:9092")
    })
    .unwrap_err();
    assert!(
        format!("{all:#}").contains("and SASL is on, TLS is on, ACLs are in force (--acl-default deny) and --listen 127.0.0.1:9092 keeps Kafka on this machine: "),
        "{all:#}"
    );
}

#[test]
fn loopback_a_turned_off_console_and_an_open_broker_on_the_network_all_start() {
    for flags in [
        with_sasl(&["--console-addr", "127.0.0.1:9093"]),
        with_sasl(&["--console-addr", "[::1]:9093"]),
        with_sasl(&["--console-addr", "localhost:9093"]),
        with_sasl(&["--console-addr", "LOCALHOST:0"]),
        with_sasl(&["--console", "--console-addr", "127.0.0.1:0"]),
        with_sasl(&["--console=false", "--console-addr", "0.0.0.0:9093"]),
        vec!["--console=false", "--console-addr", "0.0.0.0:9093"],
        vec![
            "--listen",
            "0.0.0.0:9092",
            "--console-addr",
            "127.0.0.1:9093",
        ],
        vec![],
    ] {
        let set = settings(&flags).unwrap_or_else(|e| panic!("{flags:?}: {e}"));
        assert!(
            set.warnings.iter().all(|w| !w.contains("console")),
            "{flags:?}: {:?}",
            set.warnings
        );
    }
    // Nothing guards the broker and Kafka is on the network already, so the console is no wider a door: it starts, says so, and
    // starts without the Lab.
    for listen in ["0.0.0.0:9092", "192.168.1.5:9092", "[::]:9092"] {
        let set = settings(&["--listen", listen, "--console-addr", "0.0.0.0:9093"]).unwrap();
        assert_eq!(
            set.warnings,
            [
                "the console is served on 0.0.0.0:9093 with no login and over plain HTTP: anyone who can reach that address can do what any Kafka client of this broker can (it has no login, no ACL and no TLS), and ACLs made later will not bind the console, which acts as a super user",
                "the Lab is off because the console is served beyond loopback (--lab=true switches it on)"
            ],
            "{listen}"
        );
        assert!(!set.console.lab);
    }
}

#[test]
fn unprotected_accepts_each_guard_and_the_start_says_what_it_left_open() {
    for (flags, what, exposure) in [
        (
            with_sasl(&["--console-addr", "0.0.0.0:9093", "--console-unprotected"]),
            "SASL is on, ACLs are in force (--acl-default deny) and --listen 127.0.0.1:9092 keeps Kafka on this machine",
            "act on the broker as a super user without a password, do what the ACLs refuse (the console acts as a super user) and reach from the network a broker that Kafka clients can reach only from this machine",
        ),
        (
            vec![
                "--listen",
                "0.0.0.0:9092",
                "--acl-default",
                "deny",
                "--console-addr",
                "0.0.0.0:9093",
                "--console-unprotected",
            ],
            "ACLs are in force (--acl-default deny)",
            "do what the ACLs refuse (the console acts as a super user)",
        ),
        (
            vec!["--console-addr", "0.0.0.0:9093", "--console-unprotected"],
            "--listen 127.0.0.1:9092 keeps Kafka on this machine",
            "reach from the network a broker that Kafka clients can reach only from this machine",
        ),
    ] {
        let set = settings(&flags).unwrap_or_else(|e| panic!("{flags:?}: {e}"));
        assert_eq!(
            set.warnings,
            [
                format!("the console is served on 0.0.0.0:9093 with no login and over plain HTTP although {what} (--console-unprotected): anyone who can reach that address can {exposure}"),
                "the Lab is off because the console is served beyond loopback (--lab=true switches it on)".to_owned()
            ],
            "{flags:?}"
        );
        assert_eq!(set.console.addr.as_deref(), Some("0.0.0.0:9093"));
        assert!(!set.console.lab);
    }
    let (_, tls) = crate::cli::console::setup(&crate::cli::console::Input {
        tls: true,
        unprotected: true,
        ..input("0.0.0.0:9093", "0.0.0.0:9092")
    })
    .unwrap();
    assert_eq!(
        tls[0],
        "the console is served on 0.0.0.0:9093 with no login and over plain HTTP although TLS is on (--console-unprotected): anyone who can reach that address can read topics and messages sent unencrypted"
    );
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
fn acls_stored_in_the_data_directory_refuse_a_console_beyond_loopback_once_the_broker_is_open() {
    let console = |addr: &str, unprotected: bool| Console {
        addr: Some(addr.to_owned()),
        unprotected,
        ..Console::default()
    };
    let said =
        crate::cli::console::stored_acls(&console("0.0.0.0:9093", false), true, 2).unwrap_err();
    assert_eq!(
        format!("{said:#}"),
        refusal(
            "ACLs are in force (2 stored in the data directory)",
            "0.0.0.0:9093",
            "do what the ACLs refuse (the console acts as a super user)"
        )
    );
    let note = crate::cli::console::stored_acls(&console("0.0.0.0:9093", true), true, 2).unwrap();
    assert_eq!(
        note.as_deref(),
        Some(
            "the console is served on 0.0.0.0:9093 with no login and over plain HTTP although ACLs are in force (2 stored in the data directory) (--console-unprotected): anyone who can reach that address can do what the ACLs refuse (the console acts as a super user)"
        )
    );
    // No ACLs, a console on loopback, a console that is off, or deny (the start-up rule has looked already): nothing to say.
    for (c, allow, acls) in [
        (console("0.0.0.0:9093", false), true, 0),
        (console("127.0.0.1:9093", false), true, 5),
        (
            Console {
                enabled: false,
                ..console("0.0.0.0:9093", false)
            },
            true,
            5,
        ),
        (console("0.0.0.0:9093", true), false, 5),
        (Console::default(), true, 5),
    ] {
        assert_eq!(
            crate::cli::console::stored_acls(&c, allow, acls).unwrap(),
            None,
            "{c:?}"
        );
    }
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
