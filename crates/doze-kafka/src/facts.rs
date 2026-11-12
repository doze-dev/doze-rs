//! The flags the broker was started with, as the table the console shows on Settings: every flag with its value in force, its
//! default, whether the operator gave it, and the first sentence of its own help.
//!
//! The table is made from clap's own list of arguments (`Args::command()`), so a flag added to the command line is in it with
//! no change here, and a test fails if one has no group to be listed under. What a flag carries that must not be shown is
//! never put in: the passwords of `--sasl-user` (only how many were given) and the key file of `--tls-key` (only whether one was).

use clap::parser::ValueSource;
use clap::{Arg, ArgAction, ArgMatches, Command, CommandFactory};
use kafka_console::model::SettingsFlag;

use crate::cli::Args;

/// Flags the command line accepts and does nothing with: they are not the broker's settings, so the table leaves them out.
#[cfg(test)]
pub(crate) const NOT_SHOWN: [&str; 1] = ["meta-db"];

/// What the table says for a flag whose value is a secret: how many were given, never what.
const SECRET: [&str; 1] = ["sasl-user"];

/// Flags that name a file whose name is not shown (the contents never are, and nothing here reads them).
const FILE_HIDDEN: [&str; 1] = ["tls-key"];

/// The groups the table lists the flags under, in this order, and the flags of each in this order.
pub(crate) const GROUPS: [(&str, &[&str]); 6] = [
    (
        "Network and storage",
        &[
            "listen",
            "advertised-host",
            "advertised-port",
            "data-dir",
            "segment-bytes",
            "index-interval-bytes",
            "message-max-bytes",
            "retention-bytes",
            "retention-ms",
        ],
    ),
    (
        "Topics and partitions",
        &[
            "auto-create-topics",
            "default-partitions",
            "max-partitions",
            "max-open-partitions",
        ],
    ),
    (
        "Cleaning, retention and groups",
        &[
            "log-retention-check-interval",
            "log-cleaner-interval",
            "log-cleaner-enable",
            "offsets-retention",
            "group-initial-rebalance-delay",
            "consumer-session-timeout",
            "fsync-interval",
        ],
    ),
    (
        "Profile and process",
        &[
            "kafka-version",
            "client-telemetry",
            "worker-threads",
            "log-level",
            "log-format",
        ],
    ),
    (
        "Security: sign-in, ACLs and TLS",
        &[
            "sasl-mechanisms",
            "sasl-user",
            "sasl-users-file",
            "sasl-failed-login-delay",
            "acl-default",
            "acl-super-user",
            "tls-cert",
            "tls-key",
        ],
    ),
    (
        "Console",
        &[
            "console",
            "console-addr",
            "console-prefix",
            "console-allow-host",
            "console-unprotected",
        ],
    ),
];

/// The flag as it is written on the command line, without its dashes (`data-dir`); clap's own id is the field's name.
fn long(arg: &Arg) -> &str {
    arg.get_long().unwrap_or_else(|| arg.get_id().as_str())
}

/// What the table says of a flag that has no default of its own.
fn no_default(arg: &Arg) -> &'static str {
    match long(arg) {
        "advertised-host" => "the listen address",
        "advertised-port" => "the port bound",
        "console-addr" => "127.0.0.1:9093",
        _ if matches!(arg.get_action(), ArgAction::Append) => "none",
        _ => "unset",
    }
}

/// A number that is a size or a time in milliseconds is written with thousands separators, as the page writes numbers.
fn pretty(name: &str, text: &str) -> String {
    if (name.ends_with("-bytes") || name.ends_with("-ms"))
        && let Ok(n) = text.parse::<i64>()
    {
        return kafka_console::format::number(n);
    }
    text.to_owned()
}

/// The first sentence of a flag's help.
fn first_sentence(help: &str) -> String {
    let line = help.lines().next().unwrap_or("");
    let cut = line.find(". ").map_or(line, |i| &line[..i]);
    cut.trim_end_matches('.').trim().to_owned()
}

fn default_of(arg: &Arg) -> String {
    let name = long(arg);
    arg.get_default_values().first().map_or_else(
        || no_default(arg).to_owned(),
        |d| pretty(name, &d.to_string_lossy()),
    )
}

fn value_of(arg: &Arg, matches: &ArgMatches, set: bool) -> String {
    let name = long(arg);
    if !set {
        return default_of(arg);
    }
    let given: Vec<String> = matches
        .get_raw(arg.get_id().as_str())
        .map(|v| v.map(|s| s.to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    if SECRET.contains(&name) {
        return format!("{} given", given.len());
    }
    if FILE_HIDDEN.contains(&name) {
        return "given".to_owned();
    }
    if given.is_empty() {
        return default_of(arg);
    }
    pretty(name, &given.join(", "))
}

fn flag_of(arg: &Arg, group: &str, matches: &ArgMatches) -> SettingsFlag {
    let name = long(arg);
    let set = matches!(
        matches.value_source(arg.get_id().as_str()),
        Some(ValueSource::CommandLine | ValueSource::EnvVariable)
    );
    let long = format!("--{name}");
    SettingsFlag {
        group: group.to_owned(),
        value: value_of(arg, matches, set),
        default: default_of(arg),
        set,
        what: arg
            .get_help()
            .map_or_else(String::new, |h| first_sentence(&h.to_string())),
        dynamic: kafka_console::settings_words::flag_dynamic(&long).map(str::to_owned),
        name: long,
    }
}

/// The table: every flag the command line has, grouped and ordered as [`GROUPS`] says.
pub(crate) fn flags(cmd: &Command, matches: &ArgMatches) -> Vec<SettingsFlag> {
    let mut out = Vec::new();
    for (group, names) in GROUPS {
        for name in names {
            if let Some(arg) = cmd.get_arguments().find(|a| long(a) == *name) {
                out.push(flag_of(arg, group, matches));
            }
        }
    }
    out
}

/// The names of every argument the command line has that the table must list.
#[cfg(test)]
pub(crate) fn listed_by_clap() -> Vec<String> {
    let mut cmd: Command = Args::command();
    cmd.build();
    cmd.get_arguments()
        .map(|a| long(a).to_owned())
        .filter(|n| !NOT_SHOWN.contains(&n.as_str()) && !matches!(n.as_str(), "help" | "version"))
        .collect()
}

#[cfg(test)]
mod tests;
