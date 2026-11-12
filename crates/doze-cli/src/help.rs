//! `doze --help` as a table of contents: the commands in the groups a person thinks in (what the Go help got right),
//! never an empty group (F8: Go showed "The dash:" with its only command hidden), and the flags every command takes.

use clap::Command;

/// The help groups, in order, and the commands in each.
pub const GROUPS: &[(&str, &[&str])] = &[
    ("The stack", &["up", "down", "sync", "status", "env", "run"]),
    (
        "One service",
        &[
            "wake", "sleep", "restart", "reset", "migrate", "task", "pin", "off", "on", "logs",
            "url", "connect", "why",
        ],
    ),
    (
        "Set up and edit",
        &["init", "add", "lint", "fmt", "doctor", "dns-setup", "trust"],
    ),
    ("The app", &["open"]),
    ("Modules and the lockfile", &["modules"]),
    ("Other", &["version", "update", "bug", "completion", "help"]),
];

const INTRO: &str = "\
doze runs real local services without Docker: Postgres, MariaDB, Valkey, Kvrocks,
FerretDB, Temporal, Kafka and the whole local AWS. Each wakes on its first
connection and sleeps when it is idle.

`doze` alone opens the app; over SSH it is the dash, and piped it prints the
status table. Everything the app does is also a command here, for scripts,
Makefiles and CI.";

/// The root help text, drawn from the command tree so it cannot drift from it.
pub fn root(cmd: &Command) -> String {
    let rows: Vec<(&str, String, String)> = GROUPS
        .iter()
        .flat_map(|(group, names)| {
            names.iter().map(move |name| {
                let (title, about) = match cmd.find_subcommand(name) {
                    Some(sub) => {
                        let aliases: Vec<&str> = sub.get_visible_aliases().collect();
                        let title = if aliases.is_empty() {
                            (*name).to_string()
                        } else {
                            format!("{name} ({})", aliases.join(", "))
                        };
                        (
                            title,
                            sub.get_about().map(ToString::to_string).unwrap_or_default(),
                        )
                    }
                    None => (
                        (*name).to_string(),
                        "Help for doze or one of its commands".to_string(),
                    ),
                };
                (*group, title, about)
            })
        })
        .collect();
    let flags = flag_rows(cmd);
    let col = rows
        .iter()
        .map(|(_, t, _)| t.chars().count())
        .chain(flags.iter().map(|(f, _)| f.chars().count()))
        .max()
        .unwrap_or(0)
        + 2;
    let mut out = format!("{INTRO}\n\nUsage: doze [command] [flags]\n");
    let mut current = "";
    for (group, title, about) in &rows {
        if *group != current {
            out.push_str(&format!("\n{group}:\n"));
            current = group;
        }
        out.push_str(&format!("  {}{about}\n", crate::paint::pad(title, col)));
    }
    out.push_str("\nFlags for every command:\n");
    for (flag, about) in &flags {
        out.push_str(&format!("  {}{about}\n", crate::paint::pad(flag, col)));
    }
    out.push_str("\n`doze help <command>` shows a command's own flags.\n");
    out
}

fn flag_rows(cmd: &Command) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = cmd
        .get_arguments()
        .filter(|a| a.is_global_set() && !a.is_hide_set())
        .map(|a| {
            let short = a
                .get_short()
                .map(|s| format!("-{s}, "))
                .unwrap_or_else(|| "    ".into());
            let long = a.get_long().map(|l| format!("--{l}")).unwrap_or_default();
            let value = a
                .get_value_names()
                .and_then(|v| v.first())
                .filter(|_| a.get_action().takes_values())
                .map(|v| format!(" <{v}>"))
                .unwrap_or_default();
            (
                format!("{short}{long}{value}"),
                a.get_help().map(ToString::to_string).unwrap_or_default(),
            )
        })
        .collect();
    rows.push((
        "-h, --help".into(),
        "Help (doze help <command> for one command)".into(),
    ));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// Every visible command is in exactly one group, and every group has commands that exist (F8).
    #[test]
    fn every_command_in_one_group_and_no_empty_group() {
        let cmd = crate::cli::Cli::command();
        let grouped: Vec<&str> = GROUPS.iter().flat_map(|(_, n)| n.iter().copied()).collect();
        for sub in cmd.get_subcommands().filter(|s| !s.is_hide_set()) {
            let n = grouped.iter().filter(|g| **g == sub.get_name()).count();
            assert_eq!(n, 1, "{} is in {n} groups", sub.get_name());
        }
        for (group, names) in GROUPS {
            assert!(!names.is_empty(), "{group} is empty");
            for name in *names {
                assert!(
                    *name == "help" || cmd.find_subcommand(name).is_some(),
                    "{name} is not a command"
                );
            }
        }
    }
}
