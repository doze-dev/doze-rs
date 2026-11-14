use std::collections::BTreeSet;

use clap::FromArgMatches;

use super::*;

fn table(args: &[&str]) -> Vec<SettingsFlag> {
    let mut cmd = Args::command();
    let matches = cmd
        .try_get_matches_from_mut(["doze-kafka"].into_iter().chain(args.iter().copied()))
        .unwrap();
    // The same parse the binary does: if clap would refuse the command line, so would this.
    Args::from_arg_matches(&matches).unwrap();
    flags(&cmd, &matches)
}

fn flag<'a>(t: &'a [SettingsFlag], name: &str) -> &'a SettingsFlag {
    t.iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("{name} is not in the table"))
}

/// The flags table is clap's own list: a flag added to the command line is in it, and a flag in it is on the command line.
#[test]
fn the_table_is_exactly_the_list_of_flags_clap_has_less_the_ones_that_are_accepted_and_ignored() {
    let clap: BTreeSet<String> = listed_by_clap().into_iter().collect();
    let shown: BTreeSet<String> = table(&[])
        .iter()
        .map(|f| f.name.trim_start_matches("--").to_owned())
        .collect();
    assert_eq!(
        shown, clap,
        "a flag was added to or removed from the command line: add it to GROUPS (or to NOT_SHOWN if it does nothing)"
    );
    let grouped: Vec<&str> = GROUPS
        .iter()
        .flat_map(|(_, names)| names.iter().copied())
        .collect();
    assert_eq!(
        grouped.len(),
        clap.len(),
        "a flag is listed under two groups, or under none"
    );
    // What is left out is only what the command line accepts and ignores.
    let all: BTreeSet<String> = Args::command()
        .get_arguments()
        .map(|a| long(a).to_owned())
        .collect();
    let left_out: BTreeSet<&str> = all
        .iter()
        .map(String::as_str)
        .filter(|n| !clap.contains(*n))
        .collect();
    assert!(
        left_out
            .iter()
            .all(|n| NOT_SHOWN.contains(n) || matches!(*n, "help" | "version")),
        "{left_out:?}"
    );
}

#[test]
fn every_flag_has_a_default_a_group_and_a_sentence_and_nothing_differs_when_none_is_given() {
    let t = table(&[]);
    assert!(t.len() >= 38, "{}", t.len());
    for f in &t {
        assert!(f.name.starts_with("--") && !f.group.is_empty(), "{f:?}");
        assert!(!f.what.is_empty(), "{} has no help sentence", f.name);
        assert!(!f.default.is_empty() && f.value == f.default, "{f:?}");
        assert!(!f.set && !f.differs(), "{f:?}");
    }
    assert_eq!(flag(&t, "--segment-bytes").default, "134,217,728");
    assert_eq!(flag(&t, "--listen").value, "127.0.0.1:9092");
    assert_eq!(flag(&t, "--advertised-host").value, "the listen address");
    assert_eq!(flag(&t, "--sasl-user").value, "none");
    assert_eq!(flag(&t, "--console-addr").value, "127.0.0.1:9093");
    assert_eq!(flag(&t, "--tls-cert").value, "unset");
    assert_eq!(flag(&t, "--console-unprotected").value, "false");
}

#[test]
fn a_flag_that_was_given_says_so_with_its_value_and_the_five_dynamic_ones_say_what_they_start() {
    let t = table(&[
        "--segment-bytes",
        "2097152",
        "--data-dir",
        "/srv/kafka",
        "--log-cleaner-enable=false",
        "--acl-super-user",
        "User:admin,User:root",
    ]);
    let seg = flag(&t, "--segment-bytes");
    assert_eq!(
        (
            seg.value.as_str(),
            seg.default.as_str(),
            seg.set,
            seg.differs()
        ),
        ("2,097,152", "134,217,728", true, true)
    );
    assert_eq!(seg.dynamic.as_deref(), Some("log.segment.bytes"));
    assert_eq!(flag(&t, "--data-dir").value, "/srv/kafka");
    assert_eq!(flag(&t, "--log-cleaner-enable").value, "false");
    assert!(flag(&t, "--acl-super-user").set);
    assert_eq!(t.iter().filter(|f| f.dynamic.is_some()).count(), 5);
    assert!(flag(&t, "--fsync-interval").dynamic.is_none());
    // A flag given its own default is set and does not differ.
    let same = table(&["--listen", "127.0.0.1:9092"]);
    assert!(flag(&same, "--listen").set && !flag(&same, "--listen").differs());
}

/// The passwords of `--sasl-user` and the key file of `--tls-key` are never in the table, in any field.
#[test]
fn a_password_and_a_key_file_are_never_in_the_table() {
    let t = table(&[
        "--sasl-mechanisms",
        "PLAIN",
        "--sasl-user",
        "alice:PLANTED-PASSWORD-9917",
        "--sasl-user",
        "bob:PLANTED-OTHER-2208",
        "--sasl-users-file",
        "/etc/doze/users",
        "--tls-cert",
        "/etc/doze/cert.pem",
        "--tls-key",
        "/etc/doze/PLANTED-KEY-FILE.pem",
    ]);
    let said = format!("{t:?}");
    for secret in [
        "PLANTED-PASSWORD-9917",
        "PLANTED-OTHER-2208",
        "alice",
        "bob",
        "PLANTED-KEY-FILE",
    ] {
        assert!(!said.contains(secret), "{secret} is in the table: {said}");
    }
    assert_eq!(flag(&t, "--sasl-user").value, "2 given");
    assert_eq!(flag(&t, "--tls-key").value, "given");
}

/// A new flag that carries a password cannot be forgotten: every flag whose value is named like a secret is redacted.
#[test]
fn every_flag_whose_value_is_named_like_a_secret_is_one_the_table_redacts() {
    let mut cmd = Args::command();
    cmd.build();
    for arg in cmd.get_arguments() {
        let names: Vec<String> = arg
            .get_value_names()
            .unwrap_or(&[])
            .iter()
            .map(|n| n.to_string().to_uppercase())
            .collect();
        let secretive = names
            .iter()
            .any(|n| n.contains("PASSWORD") || n.contains("SECRET") || n.contains("TOKEN"));
        let id = long(arg);
        assert_eq!(
            secretive,
            SECRET.contains(&id),
            "{id}: value names {names:?}"
        );
    }
    for id in FILE_HIDDEN {
        assert!(
            Args::command().get_arguments().any(|a| long(a) == id),
            "{id} is not a flag any more"
        );
    }
}

#[test]
fn a_help_is_cut_at_its_first_sentence() {
    assert_eq!(
        first_sentence("Serve on this host:port."),
        "Serve on this host:port"
    );
    assert_eq!(
        first_sentence("Roll a segment once it exceeds this size. More."),
        "Roll a segment once it exceeds this size"
    );
    assert_eq!(first_sentence("One line\nsecond line"), "One line");
    assert_eq!(first_sentence(""), "");
}

#[test]
fn sizes_and_times_in_milliseconds_get_separators_and_nothing_else_does() {
    assert_eq!(pretty("segment-bytes", "134217728"), "134,217,728");
    assert_eq!(pretty("retention-ms", "0"), "0");
    assert_eq!(pretty("default-partitions", "1000"), "1000");
    assert_eq!(pretty("advertised-port", "19092"), "19092");
    assert_eq!(pretty("segment-bytes", "many"), "many");
}
