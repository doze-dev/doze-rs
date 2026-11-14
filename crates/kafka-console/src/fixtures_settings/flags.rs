//! The flags of the Settings board: thirty-seven, in their groups, as the board draws them.

use crate::model::SettingsFlag;

fn flag(group: &str, name: &str, value: &str, default: &str, what: &str) -> SettingsFlag {
    SettingsFlag {
        name: name.to_owned(),
        group: group.to_owned(),
        value: value.to_owned(),
        default: default.to_owned(),
        set: false,
        what: what.to_owned(),
        dynamic: crate::settings_words::flag_dynamic(name).map(str::to_owned),
    }
}

#[allow(clippy::too_many_lines)] // a table of the boards' thirty-seven flags
pub(super) fn flags() -> Vec<SettingsFlag> {
    let n = "Network and storage";
    let t = "Topics and partitions";
    let c = "Cleaning, retention and groups";
    let p = "Profile and process";
    let s = "Security: sign-in, ACLs and TLS";
    let w = "Console";
    let mut out = vec![
        flag(
            n,
            "--listen",
            "127.0.0.1:9092",
            "127.0.0.1:9092",
            "Serve on this host and port",
        ),
        flag(
            n,
            "--advertised-host",
            "unset",
            "the listen address",
            "Host told to clients in Metadata; see Connect for the trap",
        ),
        flag(
            n,
            "--advertised-port",
            "unset",
            "the port bound",
            "Port told to clients in Metadata",
        ),
        flag(
            n,
            "--data-dir",
            "~/larder/data/kafka",
            "./data",
            "Metadata database and partition logs",
        ),
        flag(
            n,
            "--segment-bytes",
            "134,217,728",
            "134,217,728",
            "Roll a segment once it exceeds this size",
        ),
        flag(
            n,
            "--index-interval-bytes",
            "4,096",
            "4,096",
            "Add one sparse index entry per this many bytes of log",
        ),
        flag(
            n,
            "--message-max-bytes",
            "1,048,588",
            "1,048,588",
            "Largest record batch a topic accepts unless it sets max.message.bytes",
        ),
        flag(
            n,
            "--retention-bytes",
            "0",
            "0",
            "Delete old segments once a partition exceeds this size (0 = unbounded)",
        ),
        flag(
            n,
            "--retention-ms",
            "0",
            "0",
            "Delete segments older than this many milliseconds (0 = unbounded)",
        ),
        flag(
            t,
            "--auto-create-topics",
            "true",
            "true",
            "Create unknown topics when a client asks about them",
        ),
        flag(
            t,
            "--default-partitions",
            "1",
            "1",
            "Partition count for auto-created topics",
        ),
        flag(
            t,
            "--max-partitions",
            "1000",
            "1000",
            "Most partitions one topic may have",
        ),
        flag(
            t,
            "--max-open-partitions",
            "512",
            "512",
            "How many partitions stay open at once",
        ),
        flag(
            c,
            "--log-retention-check-interval",
            "5m",
            "5m",
            "How often the retention of every topic is applied",
        ),
        flag(
            c,
            "--log-cleaner-interval",
            "15s",
            "15s",
            "How often compacted topics are looked at to be cleaned",
        ),
        flag(
            c,
            "--log-cleaner-enable",
            "true",
            "true",
            "Clean topics whose cleanup.policy says compact",
        ),
        flag(
            c,
            "--offsets-retention",
            "168h",
            "168h",
            "How long committed offsets are kept once their group has no members",
        ),
        flag(
            c,
            "--group-initial-rebalance-delay",
            "200ms",
            "200ms",
            "How long the first rebalance of an empty group waits for more members",
        ),
        flag(
            c,
            "--consumer-session-timeout",
            "45s",
            "45s",
            "How long a KIP-848 member may be silent before it is removed",
        ),
        flag(
            c,
            "--fsync-interval",
            "500ms",
            "500ms",
            "How soon after an acks=1 write it is flushed",
        ),
        flag(
            p,
            "--kafka-version",
            "4",
            "4",
            "Kafka major version to stand for (1-4)",
        ),
        flag(
            p,
            "--worker-threads",
            "2",
            "2",
            "Threads running the broker's work",
        ),
        flag(
            p,
            "--log-level",
            "info",
            "info",
            "Log verbosity: error, warn, info, debug or trace",
        ),
        flag(p, "--log-format", "text", "text", "Log format"),
        flag(
            s,
            "--sasl-mechanisms",
            "none",
            "none",
            "SASL mechanisms to offer",
        ),
        flag(
            s,
            "--sasl-user",
            "none",
            "none",
            "A user that may log in, as user:password (never shown)",
        ),
        flag(
            s,
            "--sasl-users-file",
            "unset",
            "unset",
            "A file of users that may log in, one user:password a line",
        ),
        flag(
            s,
            "--sasl-failed-login-delay",
            "100ms",
            "100ms",
            "How long a failed login waits before it is answered",
        ),
        flag(
            s,
            "--acl-default",
            "allow",
            "allow",
            "What a resource that no ACL covers allows",
        ),
        flag(
            s,
            "--acl-super-user",
            "none",
            "none",
            "A principal that ACLs never refuse, as Type:name",
        ),
        flag(
            s,
            "--tls-cert",
            "unset",
            "unset",
            "PEM certificate chain (leaf first) to serve TLS with",
        ),
        flag(
            s,
            "--tls-key",
            "unset",
            "unset",
            "PEM private key of the certificate; its contents are never shown",
        ),
        flag(w, "--console", "true", "true", "The web console"),
        flag(
            w,
            "--console-addr",
            "127.0.0.1:9093",
            "127.0.0.1:9093",
            "Where the console listens",
        ),
        flag(
            w,
            "--console-prefix",
            "/",
            "/",
            "The path the console is served under",
        ),
        flag(
            w,
            "--console-allow-host",
            "none",
            "none",
            "A host name the console answers to besides localhost and IP addresses",
        ),
        flag(
            w,
            "--console-unprotected",
            "false",
            "false",
            "Accept the console beyond loopback although SASL or TLS is on",
        ),
    ];
    if let Some(f) = out.iter_mut().find(|f| f.name == "--data-dir") {
        f.set = true;
    }
    out
}
