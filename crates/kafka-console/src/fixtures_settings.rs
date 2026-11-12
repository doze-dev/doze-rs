//! The "larder" world of the Settings boards (`design/boards/settings/`): the same numbers the boards draw, as view-models, so
//! the pages can be rendered, compared with their boards and served with no broker. A test holds the headline numbers to the
//! boards' own text.

use kafka_wire::messages::ApiKey;

use crate::gather::settings_edit::{SettingsFields, call_lines};
use crate::model::{
    ConfigSource, Refusal, SettingsApi, SettingsChecked, SettingsConfigRow, SettingsConfigs,
    SettingsData, SettingsEditView, SettingsFlag, SettingsHead, SettingsInterval,
    SettingsIntervals, SettingsLine, SettingsProfile, SettingsRefused, SettingsResult,
    SettingsSaved, SettingsScope, SettingsTeach, SettingsView, SettingsWeight, Span, StopView,
};
use crate::settings_words::{DYNAMIC, allowed, family_of, takes_effect, unit, versions, what};

/// `IncrementalAlterConfigs` as the boards draw it: version 1.
const VERSION: i16 = 1;

fn row(
    key: &str,
    value: &str,
    gloss: Option<&str>,
    source: ConfigSource,
    note: &str,
) -> SettingsConfigRow {
    SettingsConfigRow {
        name: key.to_owned(),
        what: what(key).to_owned(),
        value: value.to_owned(),
        gloss: gloss.map(str::to_owned),
        source,
        source_note: note.to_owned(),
        takes_effect: takes_effect(key, Some(300)),
    }
}

/// The five rows as the Settings board draws them, with `retention_ms` for the last.
fn rows(retention: Option<&str>) -> Vec<SettingsConfigRow> {
    let last = match retention {
        Some(v) => row(
            "log.retention.ms",
            v,
            Some("3 d"),
            ConfigSource::DynamicDefault,
            "set for all brokers",
        ),
        None => row(
            "log.retention.ms",
            "-1",
            Some("no limit"),
            ConfigSource::StaticFlag,
            "flag --retention-ms; Kafka's default is 7 d",
        ),
    };
    vec![
        row(
            "log.segment.bytes",
            "134217728",
            Some("128 MiB"),
            ConfigSource::StaticFlag,
            "flag --segment-bytes; Kafka's default is 1 GiB",
        ),
        row(
            "log.index.interval.bytes",
            "4096",
            Some("4 KiB"),
            ConfigSource::Default,
            "Kafka's default",
        ),
        row(
            "message.max.bytes",
            "1048588",
            Some("1 MiB + 12 B"),
            ConfigSource::Default,
            "Kafka's default",
        ),
        row(
            "log.retention.bytes",
            "-1",
            Some("no limit"),
            ConfigSource::Default,
            "Kafka's default",
        ),
        last,
    ]
}

const APIS: [(ApiKey, i16, i16); 38] = [
    (ApiKey::Produce, 3, 9),
    (ApiKey::Fetch, 4, 13),
    (ApiKey::ListOffsets, 1, 7),
    (ApiKey::DeleteRecords, 0, 2),
    (ApiKey::ApiVersions, 0, 4),
    (ApiKey::Metadata, 0, 12),
    (ApiKey::FindCoordinator, 0, 4),
    (ApiKey::CreateTopics, 2, 7),
    (ApiKey::DeleteTopics, 1, 6),
    (ApiKey::CreatePartitions, 0, 3),
    (ApiKey::DescribeConfigs, 1, 4),
    (ApiKey::AlterConfigs, 0, 2),
    (ApiKey::IncrementalAlterConfigs, 0, 1),
    (ApiKey::JoinGroup, 0, 9),
    (ApiKey::SyncGroup, 0, 5),
    (ApiKey::Heartbeat, 0, 4),
    (ApiKey::LeaveGroup, 0, 5),
    (ApiKey::DescribeGroups, 0, 5),
    (ApiKey::ListGroups, 0, 5),
    (ApiKey::DeleteGroups, 0, 2),
    (ApiKey::ConsumerGroupHeartbeat, 0, 1),
    (ApiKey::ConsumerGroupDescribe, 0, 1),
    (ApiKey::OffsetCommit, 2, 9),
    (ApiKey::OffsetFetch, 1, 9),
    (ApiKey::OffsetDelete, 0, 0),
    (ApiKey::InitProducerId, 0, 4),
    (ApiKey::AddPartitionsToTxn, 0, 3),
    (ApiKey::AddOffsetsToTxn, 0, 3),
    (ApiKey::EndTxn, 0, 3),
    (ApiKey::TxnOffsetCommit, 0, 3),
    (ApiKey::DescribeProducers, 0, 0),
    (ApiKey::DescribeTransactions, 0, 0),
    (ApiKey::ListTransactions, 0, 1),
    (ApiKey::DescribeAcls, 1, 3),
    (ApiKey::CreateAcls, 1, 3),
    (ApiKey::DeleteAcls, 1, 3),
    (ApiKey::DescribeUserScramCredentials, 0, 0),
    (ApiKey::AlterUserScramCredentials, 0, 0),
];

fn profile() -> SettingsProfile {
    SettingsProfile {
        major: 4,
        apis: APIS
            .iter()
            .map(|(k, lo, hi)| SettingsApi {
                name: format!("{k:?}"),
                versions: versions(*lo, *hi),
                family: family_of(*k).map_or("Other", |f| f.1),
            })
            .collect(),
        with_sasl: vec![
            SettingsApi {
                name: "SaslHandshake".into(),
                versions: "v1 only".into(),
                family: "Security: ACLs and users",
            },
            SettingsApi {
                name: "SaslAuthenticate".into(),
                versions: "v0 to v2".into(),
                family: "Security: ACLs and users",
            },
        ],
        sasl_on: false,
        not_offered: vec![],
    }
}

fn interval(
    name: &'static str,
    what: &'static str,
    value: &str,
    flag: &'static str,
    note: &str,
) -> SettingsInterval {
    SettingsInterval {
        name,
        what,
        value: value.to_owned(),
        flag,
        default: value.to_owned(),
        note: note.to_owned(),
    }
}

fn intervals() -> SettingsIntervals {
    SettingsIntervals {
        rows: vec![
            interval(
                "Retention check",
                "applies retention.ms and retention.bytes to every delete topic",
                "5m",
                "--log-retention-check-interval",
                "Must be above 0",
            ),
            interval(
                "Cleaner look",
                "checks compacted topics and cleans the ones that are due",
                "15s",
                "--log-cleaner-interval",
                "Must be above 0",
            ),
            interval(
                "Flush after a write",
                "an acks=1 write is flushed within this time (0 flushes at once)",
                "500ms",
                "--fsync-interval",
                "acks=all flushes before it answers",
            ),
            interval(
                "Offsets retention",
                "committed offsets of an empty group are kept this long",
                "168h",
                "--offsets-retention",
                "kept for 7 d",
            ),
            interval(
                "First rebalance wait",
                "a new group waits this long for more members to join",
                "200ms",
                "--group-initial-rebalance-delay",
                "0 does not wait",
            ),
            interval(
                "Consumer session",
                "a KIP-848 member silent this long is removed",
                "45s",
                "--consumer-session-timeout",
                "Must be above 0",
            ),
        ],
        transactions: "500 ms".to_owned(),
        retention_every: "5 m".to_owned(),
        cleaner_every: "15 s".to_owned(),
        cleaner_on: true,
    }
}

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

fn flags() -> Vec<SettingsFlag> {
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

/// The Settings page of the larder.
#[must_use]
pub fn page() -> SettingsView {
    SettingsView {
        head: SettingsHead {
            set: 0,
            up: Some("3 d 4 h".to_owned()),
            since: Some("Nov 6 18:02 UTC".to_owned()),
            profile: "Kafka 4".to_owned(),
        },
        configs: SettingsConfigs {
            rows: rows(None),
            node_id: 1,
        },
        profile: profile(),
        intervals: intervals(),
        weight: SettingsWeight {
            open_partitions: 21,
            max_open: 64,
            flag_max_open: Some("512".to_owned()),
            open_files: Some(118),
            resident: Some(4_500_000),
            threads: Some(3),
            binary: Some(4_000_000),
            topics: 9,
            partitions: 21,
        },
        data: SettingsData {
            dir: "~/larder/data/kafka".to_owned(),
            size: Some((187 * 1024 * 1024, false)),
            files: 412,
            topics: 9,
            partitions: 21,
            last_stop: StopView::Clean,
            flag_set: true,
        },
        flags: flags(),
    }
}

fn line(text: Vec<Span>, dim: &str) -> SettingsLine {
    SettingsLine {
        text,
        dim: dim.to_owned(),
    }
}

fn view(
    key: &str,
    scope: SettingsScope,
    value: &str,
    checked: Option<SettingsChecked>,
    error: Option<&str>,
) -> SettingsEditView {
    SettingsEditView {
        key: key.to_owned(),
        what: what(key).to_owned(),
        keys: DYNAMIC.iter().map(|d| d.0.to_owned()).collect(),
        scope,
        value: value.to_owned(),
        unit: unit(key, value),
        node_id: 1,
        teach: SettingsTeach {
            now: line(
                vec![Span::text("-1, no limit.")],
                "Source: static broker, from the flag --retention-ms 0.",
            ),
            allowed: line(
                vec![Span::text(allowed(key))],
                "-1 keeps segments for ever.",
            ),
            takes_effect: line(
                vec![Span::text("at the next retention pass, every 5 m.")],
                "No restart; the change is kept on disk.",
            ),
            who_sees: line(
                vec![
                    Span::text("the topics with no retention.ms of their own: "),
                    Span::mono("payments.settled, pick-lists, pick-lists.dlq, orders.dlq"),
                ],
                "orders (7 d), notifications.email (1 d) and clickstream (1 h) keep theirs. inventory.levels and customer.profiles are compact only: retention does not apply.",
            ),
        },
        call: call_lines(VERSION, scope, 1, key, value),
        error: error.map(str::to_owned),
        checked,
    }
}

/// The edit dialog of the board: log.retention.ms, for all brokers, a value of three days typed.
#[must_use]
pub fn form(key: &str, scope: &str) -> Option<SettingsEditView> {
    crate::settings_words::is_dynamic(key).then(|| {
        let typed = if key == "log.retention.ms" {
            "259200000"
        } else {
            ""
        };
        view(key, SettingsScope::from_form(scope), typed, None, None)
    })
}

fn refusal() -> Refusal {
    Refusal {
        code: 42,
        name: "INVALID_REQUEST".to_owned(),
        message: "Cannot update these configs dynamically: [num.partitions]".to_owned(),
        rule: None,
        would_pass: None,
    }
}

fn refused(f: &SettingsFields<'_>, validate_only: bool) -> SettingsResult {
    let mut call = call_lines(VERSION, SettingsScope::All, 1, f.key, f.value);
    call.push(vec![]);
    call.push(vec![(
        crate::model::Tok::Plain,
        "-> INVALID_REQUEST (42)".to_owned(),
    )]);
    SettingsResult::Refused(Box::new(SettingsRefused {
        key: f.key.to_owned(),
        refusal: refusal(),
        rule: vec![
            Span::text("The rule: only five broker configs act on a running broker. "),
            Span::mono("num.partitions"),
            Span::text(" is read once, at start."),
        ],
        would_pass: vec![
            Span::text("Start the broker with the flag that sets it. For "),
            Span::mono("num.partitions"),
            Span::text(" that is "),
            Span::mono("--default-partitions"),
            Span::text(", now 1."),
        ],
        flag: Some("--default-partitions".to_owned()),
        call,
        others: vec![
            ("Not a Kafka config".into(), "Unknown Dynamic Configuration: foo.bar.".into(), "a name the broker does not have".into()),
            (
                "Too small or not a number".into(),
                "Invalid value 524288 for configuration log.segment.bytes: Value must be at least 1048576".into(),
                "log.segment.bytes is at least 1 MiB".into(),
            ),
            ("Another broker's id".into(), "Unexpected broker id, expected 1, but received 2".into(), "this broker is node 1".into()),
        ],
        fixed: [
            "num.partitions", "auto.create.topics.enable", "log.dirs", "listeners", "advertised.listeners",
            "log.retention.check.interval.ms", "group.initial.rebalance.delay.ms", "offsets.retention.minutes",
            "transactional.id.expiration.ms", "broker.id", "node.id",
        ]
        .map(str::to_owned)
        .to_vec(),
        validate_only,
    }))
}

/// What a submit of the larder's dialog comes to.
#[must_use]
pub fn save(f: SettingsFields<'_>, check: bool) -> SettingsResult {
    let scope = SettingsScope::from_form(f.scope);
    if !crate::settings_words::is_dynamic(f.key) {
        return refused(&f, true);
    }
    if f.was != f.key {
        return SettingsResult::Switched(Box::new(view(f.key, scope, "", None, None)));
    }
    if f.value.trim().is_empty() {
        return SettingsResult::Invalid(Box::new(view(
            f.key,
            scope,
            "",
            None,
            Some("Type a value."),
        )));
    }
    if check {
        let checked = SettingsChecked {
            before: "-1".to_owned(),
            after: f.value.trim().to_owned(),
        };
        return SettingsResult::Checked(Box::new(view(
            f.key,
            scope,
            f.value.trim(),
            Some(checked),
            None,
        )));
    }
    let mut call = call_lines(VERSION, scope, 1, f.key, f.value);
    call.push(vec![]);
    call.push(vec![(
        crate::model::Tok::Plain,
        "-> 200, no error".to_owned(),
    )]);
    SettingsResult::Saved(Box::new(SettingsSaved {
        key: f.key.to_owned(),
        scope,
        value: f.value.trim().to_owned(),
        gloss: Some("3 d".to_owned()),
        rows: rows(Some(f.value.trim())),
        set: 1,
        who_sees: vec![
            Span::text("4 topics have no retention.ms of their own and now follow "),
            Span::strong("3 d"),
            Span::text(": "),
            Span::mono("payments.settled, pick-lists, pick-lists.dlq, orders.dlq"),
            Span::text(". The topics that set their own are unchanged."),
        ],
        call,
        effect: "At the next retention pass, every 5 m".to_owned(),
    }))
}
