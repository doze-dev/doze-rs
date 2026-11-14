//! The published exit codes (Go `TestExitCodes`, `TestJSONEnvelope`, `TestPipedOutputIsPlain`), re-expressed for
//! the commands that talk to a daemon: each mistake ends with its own code, as JSON too, and piped output is plain.
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

fn run(setup: &[&str], args: &str) -> fixture::Ran {
    let lines: Vec<String> = setup.iter().map(|s| (*s).to_owned()).collect();
    let (sb, host, _d) = setup::prepare(&lines);
    let args: Vec<&str> = args.split_whitespace().collect();
    fixture::run(&sb, &host, Terminals::default(), false, &args)
}

const UP: &[&str] = &["project bare", "daemon shop"];
const DOWN: &[&str] = &["project bare"];
const DOWN_LOGS: &[&str] = &[
    "project bare",
    "logfile api r000001.s000.log 2026-10-10T09:05:00.000000Z o up",
];
const EMPTY: &[&str] = &["project bare", "daemon empty"];
const NOTHING: &[&str] = &[];

#[test]
fn each_mistake_has_its_code() {
    let cases: &[(&str, &[&str], &str, u8)] = &[
        ("a running stack's status", UP, "status", 0),
        ("an unknown command", UP, "nosuch", 2),
        ("an unknown flag", UP, "status --nosuch", 2),
        ("-w with --graph", UP, "status -w --graph", 2),
        ("--json with -q", UP, "--json -q status", 2),
        ("a zero --timeout", UP, "--timeout 0s status", 2),
        ("an undeclared service", UP, "logs nosuch", 4),
        ("an undeclared task", UP, "task nosuch", 4),
        ("waking an undeclared service", UP, "wake nosuch", 4),
        ("no config anywhere", NOTHING, "status", 6),
        ("following a stack that isn't running", DOWN, "logs -f", 7),
        ("watching a stack that isn't running", DOWN, "status -w", 7),
        ("waking a stack that isn't running", DOWN, "wake", 7),
        ("off on a stack that isn't running", DOWN, "off api", 7),
        ("reset with no terminal to ask", UP, "reset api", 9),
        ("status of a stopped stack", DOWN, "status", 0),
        ("sleep of a stopped stack", DOWN, "sleep", 0),
        ("unpinning on a stopped stack", DOWN, "pin --off api", 0),
        (
            "logs of a name nothing on disk knows, doze stopped",
            DOWN,
            "logs api",
            4,
        ),
        (
            "why of a name nothing on disk knows, doze stopped",
            DOWN,
            "why api",
            4,
        ),
        ("logs of a stopped stack", DOWN_LOGS, "logs api", 0),
        ("why on a stopped stack", DOWN_LOGS, "why api", 0),
        ("nothing to wake", EMPTY, "wake", 1),
    ];
    let daemons = setup::sockets();
    for (what, setup, args, code) in cases {
        if !daemons && setup.iter().any(|s| s.starts_with("daemon")) {
            continue;
        }
        let r = run(setup, args);
        assert_eq!(r.code, *code, "{what}: doze {args}\n{}", r.stderr);
    }
}

#[test]
fn json_carries_the_code_as_a_word_and_a_number() {
    let cases: &[(&[&str], &str, u8, &str)] = &[
        (UP, "--json logs nosuch", 4, "not_found"),
        (DOWN, "--json logs -f", 7, "daemon_down"),
        (UP, "--json reset api", 9, "needs_confirmation"),
        (NOTHING, "--json status", 6, "no_config"),
    ];
    let daemons = setup::sockets();
    for (setup, args, code, word) in cases {
        if !daemons && setup.iter().any(|s| s.starts_with("daemon")) {
            continue;
        }
        let r = run(setup, args);
        assert_eq!(r.code, *code, "doze {args}");
        let v: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
        assert_eq!(v["schema"], "doze/v1");
        assert_eq!(v["error"]["code"], *word, "doze {args}");
        assert_eq!(v["error"]["exit"], u64::from(*code), "doze {args}");
    }
    if !daemons {
        return;
    }
    // DOZE_JSON=1 asks for the envelope wherever there is a result.
    let r = run(
        &["project bare", "daemon shop", "env DOZE_JSON=1"],
        "logs web",
    );
    assert!(r.stdout.contains("\"lines\": []"), "{}", r.stdout);
}

#[test]
fn piped_output_is_plain_whatever_the_environment_says() {
    if !setup::sockets() {
        return;
    }
    for env in [
        "env TERM=xterm-256color",
        "env CLICOLOR_FORCE=1",
        "env FORCE_COLOR=1",
        "env CI=true",
    ] {
        for args in [
            "status",
            "logs api --run 2",
            "wake api",
            "reset api",
            "why cache",
            "status --graph",
        ] {
            let r = run(&["project bare", "daemon logs", env], args);
            for (name, s) in [("stdout", &r.stdout), ("stderr", &r.stderr)] {
                assert!(
                    !s.contains('\x1b'),
                    "{env} doze {args}: colour on {name}: {s:?}"
                );
            }
        }
    }
}
