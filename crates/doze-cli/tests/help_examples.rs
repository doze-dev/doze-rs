//! Every example a command's `--help` shows is a command line doze accepts: an example that drifts from the surface
//! fails here, not in a person's terminal.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/fixture.rs"]
mod fixture;

use clap::{CommandFactory, Parser};
use doze_cli::Terminals;
use doze_cli::cli::Cli;

const WITH_EXAMPLES: &[&str] = &[
    "status",
    "wake",
    "sleep",
    "restart",
    "task",
    "pin",
    "logs",
    "why",
    "doctor",
    "dns-setup",
    "trust",
    "lint",
    "fmt",
    "completion",
];

#[test]
fn every_example_in_help_parses() {
    let sb = fixture::Sandbox::new("help-examples");
    let host = fixture::FakeHost::default();
    let mut seen = 0;
    for cmd in WITH_EXAMPLES {
        let ran = fixture::run(&sb, &host, Terminals::default(), false, &[cmd, "--help"]);
        assert_eq!(ran.code, 0, "{cmd}: {}", ran.stderr);
        let (_, examples) = ran
            .stdout
            .split_once("Examples:\n")
            .unwrap_or_else(|| panic!("doze {cmd} --help has no examples:\n{}", ran.stdout));
        for line in examples
            .lines()
            .filter(|l| l.trim_start().starts_with("doze "))
        {
            // The command is what comes before the two-space gap and before a pipe.
            let line = line.trim();
            let cmdline = line
                .split("  ")
                .next()
                .unwrap()
                .split(" | ")
                .next()
                .unwrap();
            let argv: Vec<&str> = cmdline.split_whitespace().collect();
            // An example under `doze wake --help` is a `doze wake` line (aliases included).
            let sub = Cli::command();
            let named = argv
                .iter()
                .skip(1)
                .find(|a| !a.starts_with('-'))
                .and_then(|w| sub.find_subcommand(w))
                .map(|s| s.get_name().to_string());
            assert_eq!(
                named.as_deref(),
                Some(*cmd),
                "doze {cmd} --help shows {cmdline:?}"
            );
            assert!(
                Cli::try_parse_from(&argv).is_ok(),
                "doze {cmd} --help shows {cmdline:?}, which doze does not accept"
            );
            seen += 1;
        }
    }
    assert!(seen >= 30, "only {seen} examples");
}
