//! The Settings page from what the broker says: DescribeConfigs of the broker, ApiVersions' own table, the intervals and the
//! facts of this run (`ConsoleView`), the size of the data directory, and the flags the binary was started with.

use kafka_broker::{Instance, Intervals, LastStop};

use super::settings_configs::{BrokerConfigs, of_broker, set_count};
use super::settings_live::{last_round, soonest_expiry, speaking};
use crate::backend::Backend;
use crate::facts::Facts;
use crate::facts_start::StartFacts;
use crate::format::{date_time, span};
use crate::model::{
    SettingsApi, SettingsConfigs, SettingsData, SettingsHead, SettingsHeard, SettingsInterval,
    SettingsIntervals, SettingsProfile, SettingsRound, SettingsView, SettingsWeight, StopView,
};
use crate::settings_words::{family_of, flag_duration, spoken, versions};

fn api_rows(list: &[kafka_wire::profile::Advertised]) -> Vec<SettingsApi> {
    let mut rows: Vec<(usize, SettingsApi)> = list
        .iter()
        .map(|a| {
            let (place, family) = family_of(a.key).unwrap_or((usize::MAX, "Other"));
            (
                place,
                SettingsApi {
                    name: format!("{:?}", a.key),
                    versions: versions(a.min, a.max),
                    family,
                },
            )
        })
        .collect();
    rows.sort_by_key(|(place, a)| (*place, a.name.clone()));
    rows.into_iter().map(|(_, a)| a).collect()
}

/// The version profile: what ApiVersions offers now, what SASL would add, what this Kafka has that is not offered.
#[must_use]
pub fn profile(b: &Backend, facts: &Facts, heard: &[SettingsHeard]) -> SettingsProfile {
    let sasl_on = !facts.sasl.is_empty();
    let now = b.apis_offered(sasl_on);
    let with = b.apis_offered(true);
    let without = b.apis_offered(false);
    let extra: Vec<_> = with
        .iter()
        .filter(|a| !without.iter().any(|w| w.key == a.key))
        .copied()
        .collect();
    let every = b.apis_of_the_version();
    let missing: Vec<String> = every
        .iter()
        .filter(|a| !with.iter().any(|w| w.key == a.key))
        .map(|a| format!("{:?}", a.key))
        .collect();
    let offered: Vec<(String, i16, i16)> = now
        .iter()
        .map(|a| (format!("{:?}", a.key), a.min, a.max))
        .collect();
    SettingsProfile {
        speaking: speaking(heard, &offered),
        major: facts.kafka_major,
        apis: api_rows(&now),
        with_sasl: api_rows(&extra),
        sasl_on,
        not_offered: missing,
    }
}

type Row = (&'static str, &'static str, &'static str, String);

fn interval(
    start: &StartFacts,
    (name, what, flag, note): Row,
    value: std::time::Duration,
) -> SettingsInterval {
    SettingsInterval {
        name,
        what,
        value: flag_duration(value),
        flag,
        default: start
            .flag(flag)
            .map_or_else(|| "unknown".to_owned(), |f| f.default.clone()),
        note,
    }
}

/// The intervals table, from the flags' values the broker holds.
#[must_use]
pub fn intervals(
    i: &Intervals,
    start: &StartFacts,
    live: (Option<SettingsRound>, Option<(String, String)>),
) -> SettingsIntervals {
    SettingsIntervals {
        rows: interval_rows(i, start, live.1.as_ref()),
        transactions: spoken(i.transactions),
        retention_every: spoken(i.retention_check),
        cleaner_every: spoken(i.cleaner),
        cleaner_on: i.cleaner_on,
        round: live.0,
    }
}

/// The six rows of the table: each flag's value, its default and what the value comes to.
fn interval_rows(
    i: &Intervals,
    start: &StartFacts,
    soonest: Option<&(String, String)>,
) -> Vec<SettingsInterval> {
    let n = |s: &str| s.to_owned();
    vec![
        interval(
            start,
            (
                "Retention check",
                "applies retention.ms and retention.bytes to every delete topic",
                "--log-retention-check-interval",
                n("Must be above 0"),
            ),
            i.retention_check,
        ),
        interval(
            start,
            (
                "Cleaner look",
                "checks compacted topics and cleans the ones that are due",
                "--log-cleaner-interval",
                n("Must be above 0"),
            ),
            i.cleaner,
        ),
        interval(
            start,
            (
                "Flush after a write",
                "an acks=1 write is flushed within this time (0 flushes at once)",
                "--fsync-interval",
                n("acks=all flushes before it answers"),
            ),
            i.fsync,
        ),
        interval(
            start,
            (
                "Offsets retention",
                "committed offsets of an empty group are kept this long",
                "--offsets-retention",
                offsets_note(i, soonest),
            ),
            i.offsets_retention,
        ),
        interval(
            start,
            (
                "First rebalance wait",
                "a new group waits this long for more members to join",
                "--group-initial-rebalance-delay",
                n("0 does not wait"),
            ),
            i.group_initial_rebalance,
        ),
        interval(
            start,
            (
                "Consumer session",
                "a KIP-848 member silent this long is removed",
                "--consumer-session-timeout",
                n("Must be above 0"),
            ),
            i.consumer_session,
        ),
    ]
}

/// The offsets-retention row says how long that is in words, from the value itself, and which group's offsets the sweep takes
/// first and in how long (`soonest`: the group and the span, where some group has a date).
#[must_use]
pub fn offsets_note(i: &Intervals, soonest: Option<&(String, String)>) -> String {
    let kept = if i.offsets_retention.is_zero() {
        "kept for ever".to_owned()
    } else {
        format!(
            "kept for {}",
            span(i64::try_from(i.offsets_retention.as_secs()).unwrap_or(i64::MAX))
        )
    };
    match soonest {
        Some((group, within)) => {
            // "analytics' expire", "payments' expire", "orders-api's expire".
            let own = if group.ends_with('s') { "'" } else { "'s" };
            format!("{kept}; {group}{own} expire in {within}")
        }
        None => kept,
    }
}

fn stop_of(s: LastStop) -> StopView {
    match s {
        LastStop::None => StopView::None,
        LastStop::Clean => StopView::Clean,
        LastStop::Unclean => StopView::Unclean,
    }
}

/// The data directory as an absolute path where the system can say it, else as given.
fn absolute(p: &std::path::Path) -> String {
    std::fs::canonicalize(p)
        .unwrap_or_else(|_| p.to_path_buf())
        .display()
        .to_string()
}

/// The binary's size, read from the running program's own file.
fn binary_size() -> Option<u64> {
    std::fs::metadata(std::env::current_exe().ok()?)
        .ok()
        .map(|m| m.len())
}

fn weight(i: Option<&Instance>, start: &StartFacts, counts: (usize, usize)) -> SettingsWeight {
    SettingsWeight {
        open_partitions: i.map_or(0, |i| i.open_partitions),
        max_open: i.map_or(0, |i| i.max_open_partitions),
        flag_max_open: start.value("--max-open-partitions").map(str::to_owned),
        open_files: i.and_then(|i| i.process.open_files),
        resident: i.and_then(|i| i.process.resident_bytes),
        threads: i.and_then(|i| i.process.threads),
        binary: binary_size(),
        topics: counts.0,
        partitions: counts.1,
    }
}

/// The page.
pub async fn settings(
    b: &Backend,
    (facts, start): (&Facts, &StartFacts),
    heard: &[SettingsHeard],
    now_ms: i64,
) -> SettingsView {
    let instance = b.instance();
    let counts = super::counts(b).await;
    let ints = b.intervals();
    let retention = ints.as_ref().map(|i| i.retention_check.as_secs());
    let node = ints.as_ref().map_or(1, |i| i.node_id);
    let configs = of_broker(b, node, retention)
        .await
        .unwrap_or_else(BrokerConfigs::default);
    let size = b.data_size().await;
    let head = SettingsHead {
        set: set_count(&configs.rows),
        up: instance
            .as_ref()
            .map(|i| span((now_ms - i.started_ms).max(0) / 1000)),
        since: instance.as_ref().map(|i| date_time(i.started_ms)),
        profile: format!("Kafka {}", facts.kafka_major),
    };
    let live = (last_round(b).await, soonest_expiry(b, now_ms).await);
    let rows = ints.as_ref().map_or_else(
        || SettingsIntervals {
            rows: vec![],
            transactions: String::new(),
            retention_every: String::new(),
            cleaner_every: String::new(),
            cleaner_on: true,
            round: None,
        },
        |i| intervals(i, start, live.clone()),
    );
    SettingsView {
        head,
        configs: SettingsConfigs {
            rows: configs.rows,
            node_id: node,
        },
        profile: profile(b, facts, heard),
        intervals: rows,
        weight: weight(instance.as_ref(), start, counts),
        data: SettingsData {
            dir: instance
                .as_ref()
                .map_or_else(String::new, |i| absolute(&i.data_dir)),
            size: size.map(|s| (s.bytes, s.cut)),
            files: size.map_or(0, |s| s.files),
            topics: counts.0,
            partitions: counts.1,
            last_stop: instance
                .as_ref()
                .map_or(StopView::None, |i| stop_of(i.last_stop)),
            flag_set: start.flag("--data-dir").is_some_and(|f| f.set),
        },
        flags: start.flags.clone(),
    }
}

#[cfg(test)]
mod tests;
