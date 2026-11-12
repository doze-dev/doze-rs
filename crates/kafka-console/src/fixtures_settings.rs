//! The "larder" world of the Settings boards (`design/boards/settings/`): the same numbers the boards draw, as view-models, so
//! the pages can be rendered, compared with their boards and served with no broker. A test holds the headline numbers to the
//! boards' own text.

mod flags;
mod profile;

use crate::gather::settings_edit::{SettingsFields, call_lines};
use crate::model::{
    ConfigSource, Refusal, SettingsChecked, SettingsConfigRow, SettingsConfigs, SettingsData,
    SettingsEditView, SettingsHead, SettingsInterval, SettingsIntervals, SettingsLine,
    SettingsRefused, SettingsResult, SettingsRound, SettingsSaved, SettingsScope, SettingsTeach,
    SettingsView, SettingsWeight, Span, StopView,
};
use crate::settings_words::{DYNAMIC, allowed, takes_effect, unit, what};

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
                "kept for 7 d; analytics' expire in 3 d 2 h",
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
        round: Some(SettingsRound {
            topic: "inventory.levels".to_owned(),
            partition: 0,
            at: "21:31".to_owned(),
            due: Some(true),
        }),
    }
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
        profile: profile::profile(),
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
        flags: flags::flags(),
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
                    Span::text("."),
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
