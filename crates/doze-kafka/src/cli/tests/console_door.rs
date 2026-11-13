//! The start-up rule on where the console is served (SEC-188, SEC-189): the console is never a wider door than the broker.

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
