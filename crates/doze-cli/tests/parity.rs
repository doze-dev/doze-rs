//! The verb table (`doze_control::actions`) and the command surface agree both ways (Go `TestActionRegistryParity`):
//! every verb meant for the CLI is a command (by its name; its aliases are the command's too), and every command that
//! acts on a stack is in the table. Adding a verb to one side only fails here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use clap::CommandFactory;
use doze_cli::cli::Cli;
use doze_control::actions;
use std::collections::BTreeSet;

/// Commands that are faces or plumbing, not verbs on a stack.
const META: &[&str] = &[
    "open",
    "modules",
    "version",
    "dns-setup",
    "completion",
    "help",
    "update",
    "bug",
];

#[test]
fn every_verb_is_a_command_and_every_command_a_verb() {
    let root = Cli::command();
    let mut names = BTreeSet::new();
    for c in root.get_subcommands() {
        names.insert(c.get_name().to_owned());
        for a in c.get_all_aliases() {
            names.insert(a.to_owned());
        }
    }
    for a in actions::ALL.iter().filter(|a| a.cli) {
        assert!(names.contains(a.name), "verb {:?} has no command", a.name);
        let cmd = root.find_subcommand(a.name).unwrap();
        for alias in a.aliases {
            // `sync`'s `apply` is the daemon's op name, not a spelling people type.
            if *alias == "apply" {
                continue;
            }
            assert!(
                cmd.get_all_aliases().any(|x| x == *alias),
                "{:?} has no alias {alias:?}",
                a.name
            );
        }
    }
    for c in root.get_subcommands() {
        if c.is_hide_set() || META.contains(&c.get_name()) {
            continue;
        }
        assert!(
            actions::lookup(c.get_name()).is_some(),
            "command {:?} is not in the verb table",
            c.get_name()
        );
    }
}
