//! The streams group configs (KIP-1071): their names, what a streams group runs by, and the broker's bounds on a group's
//! own value (Kafka 4.4.0's `GroupConfig.validateValues`, at the `group.streams.*` broker defaults).

use std::collections::BTreeMap;

use kafka_wire::ResponseError as E;

use super::{Refusal, refuse};

pub(crate) const STREAMS_SESSION: &str = "streams.session.timeout.ms";
pub(crate) const STREAMS_HEARTBEAT: &str = "streams.heartbeat.interval.ms";
pub(crate) const STANDBYS: &str = "streams.num.standby.replicas";
pub(crate) const INITIAL_DELAY: &str = "streams.initial.rebalance.delay.ms";
pub(crate) const STREAMS_ASSIGN: &str = "streams.assignment.interval.ms";
pub(crate) const OFFLOAD: &str = "streams.assignor.offload.enable";
pub(crate) const TASK_OFFSETS: &str = "streams.task.offset.interval.ms";
pub(crate) const WARMUPS: &str = "streams.num.warmup.replicas";
pub(crate) const TAGS: &str = "streams.rack.aware.assignment.tags";
pub(crate) const ACCEPTABLE_LAG: &str = "streams.acceptable.recovery.lag";
pub(crate) const ASSIGNOR: &str = "streams.assignor.name";

/// The one task assignor there is (`group.streams.assignors`'s default).
pub(crate) const STICKY: &str = "sticky";

/// The broker's `group.streams.*` configs, at Kafka 4.4.0's defaults.
pub(crate) mod defaults {
    pub(crate) const SESSION_MS: i64 = 45_000;
    pub(crate) const SESSION_MS_RANGE: (i64, i64) = (45_000, 60_000);
    pub(crate) const HEARTBEAT_MS: i64 = 5_000;
    pub(crate) const HEARTBEAT_MS_RANGE: (i64, i64) = (5_000, 15_000);
    pub(crate) const MAX_STANDBYS: i64 = 2;
    pub(crate) const INITIAL_DELAY_MS: i64 = 3_000;
    pub(crate) const ASSIGN_INTERVAL_MS: i64 = 1_000;
    pub(crate) const ASSIGN_INTERVAL_MS_RANGE: (i64, i64) = (0, 15_000);
    pub(crate) const TASK_OFFSET_MS: i64 = 60_000;
    pub(crate) const MIN_TASK_OFFSET_MS: i64 = 15_000;
    pub(crate) const MAX_WARMUPS: i64 = 20;
    pub(crate) const ACCEPTABLE_LAG: i64 = 10_000;
    /// `group.streams.max.size`.
    pub(crate) const MAX_SIZE: usize = i32::MAX as usize;
}

/// What a streams group runs by: its own configs where set, else the broker's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StreamsSettings {
    pub(crate) session_ms: i64,
    pub(crate) heartbeat_ms: i64,
    pub(crate) standbys: i64,
    pub(crate) initial_delay_ms: i64,
    pub(crate) assign_interval_ms: i64,
    pub(crate) task_offset_ms: i64,
    pub(crate) acceptable_lag: i64,
    /// The client tags rack-aware standby assignment is to use (none by default).
    pub(crate) tags: Vec<String>,
}

impl Default for StreamsSettings {
    fn default() -> Self {
        Self {
            session_ms: defaults::SESSION_MS,
            heartbeat_ms: defaults::HEARTBEAT_MS,
            standbys: 0,
            initial_delay_ms: defaults::INITIAL_DELAY_MS,
            assign_interval_ms: defaults::ASSIGN_INTERVAL_MS,
            task_offset_ms: defaults::TASK_OFFSET_MS,
            acceptable_lag: defaults::ACCEPTABLE_LAG,
            tags: Vec::new(),
        }
    }
}

impl StreamsSettings {
    /// The settings of a group with these configs (each already valid: it was checked when it was set).
    #[must_use]
    pub(crate) fn of(configs: &BTreeMap<String, String>) -> Self {
        let mut s = Self::default();
        let int = |name: &str, into: &mut i64| {
            if let Some(v) = configs.get(name).and_then(|v| v.trim().parse().ok()) {
                *into = v;
            }
        };
        int(STREAMS_SESSION, &mut s.session_ms);
        int(STREAMS_HEARTBEAT, &mut s.heartbeat_ms);
        int(STANDBYS, &mut s.standbys);
        int(INITIAL_DELAY, &mut s.initial_delay_ms);
        int(STREAMS_ASSIGN, &mut s.assign_interval_ms);
        int(TASK_OFFSETS, &mut s.task_offset_ms);
        int(ACCEPTABLE_LAG, &mut s.acceptable_lag);
        if let Some(v) = configs.get(TAGS) {
            s.tags = list(v);
        }
        s
    }
}

/// A LIST config's items: split on commas, trimmed, empty ones dropped.
pub(crate) fn list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Kafka's checks of a streams config's own value that its kind does not cover: no tag twice, and a known assignor.
pub(super) fn check_value(name: &str, value: &str) -> Result<(), Refusal> {
    if name == TAGS {
        let tags = list(value);
        if tags
            .iter()
            .enumerate()
            .any(|(i, t)| tags.iter().take(i).any(|u| u == t))
        {
            return Err(refuse(
                E::InvalidConfig,
                format!("{TAGS} must not contain duplicate tag keys."),
            ));
        }
    }
    if name == ASSIGNOR && value != STICKY {
        return Err(refuse(
            E::InvalidConfig,
            format!(
                "{ASSIGNOR} '{value}' is not a registered task assignor. Registered assignors are: [{STICKY}]."
            ),
        ));
    }
    Ok(())
}

/// A bound a group's value is held to.
enum Bound {
    Range(i64, i64),
    Max(i64),
    Min(i64),
}

/// The bounds against the broker's configs, on the keys the request sets, in Kafka's order; then the session against the
/// heartbeat, each side the group's or the broker's.
pub(super) fn check_bounds(next: &BTreeMap<String, String>, set: &[&str]) -> Result<(), Refusal> {
    let (hb, session, assign) = (
        defaults::HEARTBEAT_MS_RANGE,
        defaults::SESSION_MS_RANGE,
        defaults::ASSIGN_INTERVAL_MS_RANGE,
    );
    let bounds = [
        (STREAMS_HEARTBEAT, Bound::Range(hb.0, hb.1)),
        (STREAMS_SESSION, Bound::Range(session.0, session.1)),
        (STANDBYS, Bound::Max(defaults::MAX_STANDBYS)),
        (STREAMS_ASSIGN, Bound::Range(assign.0, assign.1)),
        (TASK_OFFSETS, Bound::Min(defaults::MIN_TASK_OFFSET_MS)),
        (WARMUPS, Bound::Max(defaults::MAX_WARMUPS)),
    ];
    for (name, bound) in bounds {
        let Some(n) = set
            .contains(&name)
            .then(|| next.get(name).and_then(|v| v.trim().parse::<i64>().ok()))
            .flatten()
        else {
            continue;
        };
        let words = match bound {
            Bound::Range(lo, hi) if !(lo..=hi).contains(&n) => {
                format!("{name} must be in the range {lo} to {hi} inclusive.")
            }
            Bound::Max(hi) if n > hi => format!("{name} must be less than or equal to {hi}"),
            Bound::Min(lo) if n < lo => format!("{name} must be greater than or equal to {lo}"),
            _ => continue,
        };
        return Err(refuse(E::InvalidConfig, words));
    }
    let s = StreamsSettings::of(next);
    if s.session_ms <= s.heartbeat_ms {
        return Err(refuse(
            E::InvalidConfig,
            format!("{STREAMS_SESSION} must be greater than {STREAMS_HEARTBEAT}"),
        ));
    }
    Ok(())
}
