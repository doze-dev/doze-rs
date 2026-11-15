//! The configs of a group (Kafka's `GroupConfig`, resource type GROUP): the share group ones, which say how a share group's
//! records are delivered and how its members are timed. A group need not exist to have them, and a value set is the
//! group's until it is deleted; what is not set is the broker's default (Kafka's `group.share.*` broker configs, at
//! their defaults here).
//!
//! The `consumer.*`, `streams.*` and dead-letter-queue group configs Kafka also has are not kept: setting one is refused
//! by name (docs/port/behaviours/share-groups.md, SG-D5).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, PoisonError, RwLock};

use kafka_wire::ResponseError as E;
use kafka_wire::ids::GroupId;

use crate::configs::{Change, Op, Refusal};

/// Kafka's broker defaults of the share group configs, and the bounds a group's own value is held to
/// (`ShareGroupConfig`, `GroupCoordinatorConfig`).
pub(crate) mod defaults {
    pub(crate) const SESSION_MS: i64 = 45_000;
    pub(crate) const SESSION_MS_RANGE: (i64, i64) = (45_000, 60_000);
    pub(crate) const HEARTBEAT_MS: i64 = 5_000;
    pub(crate) const HEARTBEAT_MS_RANGE: (i64, i64) = (5_000, 15_000);
    pub(crate) const LOCK_MS: i64 = 30_000;
    pub(crate) const LOCK_MS_RANGE: (i64, i64) = (15_000, 60_000);
    pub(crate) const DELIVERY_LIMIT: i64 = 5;
    pub(crate) const DELIVERY_LIMIT_RANGE: (i64, i64) = (2, 10);
    pub(crate) const RECORD_LOCKS: i64 = 2_000;
    pub(crate) const RECORD_LOCKS_RANGE: (i64, i64) = (100, 4_000);
    pub(crate) const ASSIGN_INTERVAL_MS: i64 = 1_000;
    pub(crate) const ASSIGN_INTERVAL_MS_RANGE: (i64, i64) = (0, 15_000);
    /// `group.share.max.size`.
    pub(crate) const MAX_SIZE: usize = 200;
    /// `group.share.max.share.sessions`.
    pub(crate) const MAX_SESSIONS: usize = 2_000;
}

pub(crate) const SESSION: &str = "share.session.timeout.ms";
pub(crate) const HEARTBEAT: &str = "share.heartbeat.interval.ms";
pub(crate) const LOCK: &str = "share.record.lock.duration.ms";
pub(crate) const DELIVERY: &str = "share.delivery.count.limit";
pub(crate) const LOCKS: &str = "share.partition.max.record.locks";
pub(crate) const RESET: &str = "share.auto.offset.reset";
pub(crate) const ISOLATION: &str = "share.isolation.level";
pub(crate) const RENEW: &str = "share.renew.acknowledge.enable";
pub(crate) const ASSIGN: &str = "share.assignment.interval.ms";

/// The kind of value a config takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Int { at_least: i64 },
    Text,
    Bool,
}

/// One group config: its name, kind, Kafka's default as text, and its broker synonym.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Def {
    pub(crate) name: &'static str,
    pub(crate) kind: Kind,
    pub(crate) default: &'static str,
    pub(crate) synonym: Option<&'static str>,
}

/// The group configs kept, in Kafka's order of definition.
pub(crate) const DEFS: &[Def] = &[
    Def { name: SESSION, kind: Kind::Int { at_least: 1 }, default: "45000", synonym: Some("group.share.session.timeout.ms") },
    Def { name: HEARTBEAT, kind: Kind::Int { at_least: 1 }, default: "5000", synonym: Some("group.share.heartbeat.interval.ms") },
    Def { name: LOCK, kind: Kind::Int { at_least: 1000 }, default: "30000", synonym: Some("group.share.record.lock.duration.ms") },
    Def { name: DELIVERY, kind: Kind::Int { at_least: 2 }, default: "5", synonym: Some("group.share.delivery.count.limit") },
    Def { name: LOCKS, kind: Kind::Int { at_least: 100 }, default: "2000", synonym: Some("group.share.partition.max.record.locks") },
    Def { name: RESET, kind: Kind::Text, default: "latest", synonym: None },
    Def { name: ISOLATION, kind: Kind::Text, default: "read_uncommitted", synonym: None },
    Def { name: RENEW, kind: Kind::Bool, default: "true", synonym: None },
    Def { name: ASSIGN, kind: Kind::Int { at_least: 0 }, default: "1000", synonym: Some("group.share.assignment.interval.ms") },
];

/// Group configs Kafka has that this broker does not keep.
const NOT_KEPT: &[&str] = &[
    "consumer.session.timeout.ms",
    "consumer.heartbeat.interval.ms",
    "consumer.assignment.interval.ms",
    "streams.session.timeout.ms",
    "streams.heartbeat.interval.ms",
    "streams.num.standby.replicas",
    "streams.initial.rebalance.delay.ms",
    "streams.assignment.interval.ms",
    "streams.rack.aware.assignment.tags",
    "streams.task.offset.interval.ms",
    "streams.num.warmup.replicas",
    "streams.acceptable.recovery.lag",
    "streams.assignor.name",
    "errors.deadletterqueue.topic.name",
    "errors.deadletterqueue.copy.record.enable",
];

/// Where a share-partition starts when its group has no start for it yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reset {
    Earliest,
    Latest,
    /// The first record no older than this many milliseconds.
    ByDuration(i64),
}

/// What a share group runs by: its own configs where set, else the broker's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShareSettings {
    pub(crate) session_ms: i64,
    pub(crate) heartbeat_ms: i64,
    pub(crate) lock_ms: i64,
    pub(crate) delivery_limit: i64,
    pub(crate) record_locks: i64,
    pub(crate) reset: Reset,
    pub(crate) read_committed: bool,
    pub(crate) renew: bool,
    pub(crate) assign_interval_ms: i64,
}

impl Default for ShareSettings {
    fn default() -> Self {
        Self {
            session_ms: defaults::SESSION_MS,
            heartbeat_ms: defaults::HEARTBEAT_MS,
            lock_ms: defaults::LOCK_MS,
            delivery_limit: defaults::DELIVERY_LIMIT,
            record_locks: defaults::RECORD_LOCKS,
            reset: Reset::Latest,
            read_committed: false,
            renew: true,
            assign_interval_ms: defaults::ASSIGN_INTERVAL_MS,
        }
    }
}

impl ShareSettings {
    /// The settings of a group with these configs (each already valid: it was checked when it was set).
    #[must_use]
    pub(crate) fn of(configs: &BTreeMap<String, String>) -> Self {
        let mut s = Self::default();
        let int = |name: &str, into: &mut i64| {
            if let Some(v) = configs.get(name).and_then(|v| v.trim().parse().ok()) {
                *into = v;
            }
        };
        int(SESSION, &mut s.session_ms);
        int(HEARTBEAT, &mut s.heartbeat_ms);
        int(LOCK, &mut s.lock_ms);
        int(DELIVERY, &mut s.delivery_limit);
        int(LOCKS, &mut s.record_locks);
        int(ASSIGN, &mut s.assign_interval_ms);
        if let Some(r) = configs.get(RESET).and_then(|v| parse_reset(v)) {
            s.reset = r;
        }
        s.read_committed = configs.get(ISOLATION).is_some_and(|v| v == "read_committed");
        if let Some(v) = configs.get(RENEW) {
            s.renew = v.trim().eq_ignore_ascii_case("true");
        }
        s
    }
}

/// `earliest`, `latest` or `by_duration:<ISO-8601 duration>` (Kafka's `ShareGroupAutoOffsetResetStrategy`).
#[must_use]
pub(crate) fn parse_reset(v: &str) -> Option<Reset> {
    let v = v.trim().to_ascii_lowercase();
    match v.as_str() {
        "earliest" => Some(Reset::Earliest),
        "latest" => Some(Reset::Latest),
        other => other
            .strip_prefix("by_duration:")
            .and_then(iso_duration_ms)
            .map(Reset::ByDuration),
    }
}

/// An ISO-8601 duration as Java's `Duration.parse` takes it, `PnDTnHnMn.nS`, to milliseconds; negatives are refused.
fn iso_duration_ms(text: &str) -> Option<i64> {
    let rest = text.strip_prefix('p')?;
    let (days, time) = match rest.split_once('t') {
        Some((d, t)) => (d, Some(t)),
        None => (rest, None),
    };
    let mut ms: i64 = 0;
    if !days.is_empty() {
        let n: i64 = days.strip_suffix('d')?.parse().ok()?;
        ms = ms.checked_add(n.checked_mul(86_400_000)?)?;
    }
    let Some(mut time) = time else {
        return (!days.is_empty() && ms >= 0).then_some(ms);
    };
    if time.is_empty() {
        return None;
    }
    for (unit, scale) in [('h', 3_600_000_f64), ('m', 60_000_f64), ('s', 1_000_f64)] {
        if let Some(i) = time.find(unit) {
            let n: f64 = time.get(..i)?.parse().ok()?;
            if n < 0.0 || (unit != 's' && n.fract() != 0.0) {
                return None;
            }
            // Whole milliseconds: a fraction of one is dropped, as Java's toMillis does.
            #[expect(clippy::cast_possible_truncation, reason = "checked to be a non-negative finite number of ms")]
            let part = (n * scale) as i64;
            ms = ms.checked_add(part)?;
            time = time.get(i + 1..)?;
        }
    }
    (time.is_empty() && ms >= 0).then_some(ms)
}

fn refuse(code: E, message: impl Into<String>) -> Refusal {
    Refusal {
        code,
        message: message.into(),
    }
}

fn invalid(value: &str, name: &str, detail: &str) -> Refusal {
    refuse(
        E::InvalidConfig,
        format!("Invalid value {value} for configuration {name}: {detail}"),
    )
}

/// One value checked against its config's kind (Kafka's `ConfigDef` parse and validators).
fn check_value(def: &Def, value: &str) -> Result<(), Refusal> {
    match def.kind {
        Kind::Int { at_least } => {
            let n: i64 = value
                .trim()
                .parse::<i32>()
                .map(i64::from)
                .map_err(|_| invalid(value, def.name, "Not a number of type INT"))?;
            if n < at_least {
                return Err(invalid(value, def.name, &format!("Value must be at least {at_least}")));
            }
        }
        Kind::Bool => {
            let v = value.trim();
            if !v.eq_ignore_ascii_case("true") && !v.eq_ignore_ascii_case("false") {
                return Err(invalid(value, def.name, "Expected value to be either true or false"));
            }
        }
        Kind::Text if def.name == ISOLATION => {
            if value != "read_committed" && value != "read_uncommitted" {
                return Err(invalid(
                    value,
                    def.name,
                    "String must be one of: read_committed, read_uncommitted",
                ));
            }
        }
        Kind::Text => {
            if parse_reset(value).is_none() {
                return Err(refuse(
                    E::InvalidConfig,
                    format!(
                        "Invalid value `{value}` for configuration {RESET}. The value must be either 'earliest', 'latest' or of the format 'by_duration:<PnDTnHnMn.nS>'."
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// The bounds against the broker's configs (Kafka's `GroupConfig.validateValues`), on the keys the request sets.
fn check_bounds(next: &BTreeMap<String, String>, set: &[&str]) -> Result<(), Refusal> {
    let bounds = [
        (HEARTBEAT, defaults::HEARTBEAT_MS_RANGE),
        (SESSION, defaults::SESSION_MS_RANGE),
        (LOCK, defaults::LOCK_MS_RANGE),
        (DELIVERY, defaults::DELIVERY_LIMIT_RANGE),
        (LOCKS, defaults::RECORD_LOCKS_RANGE),
        (ASSIGN, defaults::ASSIGN_INTERVAL_MS_RANGE),
    ];
    for (name, (lo, hi)) in bounds {
        if !set.contains(&name) {
            continue;
        }
        if let Some(n) = next.get(name).and_then(|v| v.trim().parse::<i64>().ok())
            && !(lo..=hi).contains(&n)
        {
            return Err(refuse(
                E::InvalidConfig,
                format!("{name} must be in the range {lo} to {hi} inclusive."),
            ));
        }
    }
    let s = ShareSettings::of(next);
    if s.session_ms <= s.heartbeat_ms {
        return Err(refuse(
            E::InvalidConfig,
            format!("{SESSION} must be greater than {HEARTBEAT}"),
        ));
    }
    Ok(())
}

/// The configs a group has after `changes` (Kafka's `ControllerConfigurationValidator` and `GroupConfig.validate`).
///
/// # Errors
/// INVALID_REQUEST for an empty group name or a value missing, INVALID_CONFIG for a name not known or a value not taken.
pub(crate) fn apply(
    group: &str,
    current: &BTreeMap<String, String>,
    changes: &[Change],
    replace: bool,
) -> Result<BTreeMap<String, String>, Refusal> {
    if group.is_empty() {
        return Err(refuse(E::InvalidRequest, "Default group resources are not allowed."));
    }
    let nulls: Vec<&str> = changes
        .iter()
        .filter(|c| matches!(c.op, Op::Set) && c.value.is_none())
        .map(|c| c.name.as_str())
        .collect();
    if !nulls.is_empty() {
        return Err(refuse(
            E::InvalidRequest,
            format!("Null value not supported for group configs: {}", nulls.join(",")),
        ));
    }
    let mut next = if replace { BTreeMap::new() } else { current.clone() };
    let mut set: Vec<&str> = Vec::new();
    for c in changes {
        let Some(def) = DEFS.iter().find(|d| d.name == c.name) else {
            let message = if NOT_KEPT.contains(&c.name.as_str()) {
                format!("Group config {} is not supported by this broker: it keeps the share.* group configs only.", c.name)
            } else {
                format!("Unknown group config name: {}", c.name)
            };
            return Err(refuse(E::InvalidConfig, message));
        };
        match (&c.op, &c.value) {
            (Op::Set, Some(v)) => {
                check_value(def, v)?;
                next.insert(c.name.clone(), v.clone());
                set.push(def.name);
            }
            (Op::Delete, _) => {
                next.remove(&c.name);
            }
            (Op::Append | Op::Subtract, _) => {
                return Err(refuse(
                    E::InvalidConfig,
                    format!("Config value append is not allowed for config key: {}", c.name),
                ));
            }
            (Op::Set, None) => {}
        }
    }
    check_bounds(&next, &set)?;
    Ok(next)
}

/// Every group's configs, as stored, and the settings they make.
#[derive(Debug, Default)]
pub(crate) struct GroupConfigs(RwLock<HashMap<GroupId, Arc<BTreeMap<String, String>>>>);

impl GroupConfigs {
    pub(crate) fn load(all: BTreeMap<GroupId, BTreeMap<String, String>>) -> Self {
        Self(RwLock::new(all.into_iter().map(|(g, c)| (g, Arc::new(c))).collect()))
    }

    /// The configs set on `group` (none, if none is).
    #[must_use]
    pub(crate) fn of(&self, group: &GroupId) -> Arc<BTreeMap<String, String>> {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(group)
            .cloned()
            .unwrap_or_default()
    }

    /// What `group` runs by.
    #[must_use]
    pub(crate) fn settings(&self, group: &GroupId) -> ShareSettings {
        ShareSettings::of(&self.of(group))
    }

    pub(crate) fn set(&self, group: &GroupId, configs: BTreeMap<String, String>) {
        let mut all = self.0.write().unwrap_or_else(PoisonError::into_inner);
        if configs.is_empty() {
            all.remove(group);
        } else {
            all.insert(group.clone(), Arc::new(configs));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(name: &str, value: &str) -> Change {
        Change {
            name: name.into(),
            op: Op::Set,
            value: Some(value.into()),
        }
    }

    fn refused(changes: &[Change]) -> (E, String) {
        let r = apply("g", &BTreeMap::new(), changes, false).unwrap_err();
        (r.code, r.message)
    }

    #[test]
    fn a_value_is_checked_by_kind_then_against_the_brokers_bounds_in_kafkas_words() {
        assert_eq!(
            refused(&[set(DELIVERY, "x")]),
            (E::InvalidConfig, "Invalid value x for configuration share.delivery.count.limit: Not a number of type INT".into())
        );
        assert_eq!(
            refused(&[set(DELIVERY, "1")]).1,
            "Invalid value 1 for configuration share.delivery.count.limit: Value must be at least 2"
        );
        assert_eq!(
            refused(&[set(DELIVERY, "11")]).1,
            "share.delivery.count.limit must be in the range 2 to 10 inclusive."
        );
        assert_eq!(
            refused(&[set(LOCK, "10000")]).1,
            "share.record.lock.duration.ms must be in the range 15000 to 60000 inclusive."
        );
        assert_eq!(
            refused(&[set(ISOLATION, "READ_COMMITTED")]).1,
            "Invalid value READ_COMMITTED for configuration share.isolation.level: String must be one of: read_committed, read_uncommitted"
        );
        assert_eq!(refused(&[set("nope", "1")]).1, "Unknown group config name: nope");
        assert!(refused(&[set("consumer.session.timeout.ms", "50000")]).1.contains("not supported"));
        assert_eq!(
            apply("", &BTreeMap::new(), &[], false).unwrap_err().message,
            "Default group resources are not allowed."
        );
    }

    #[test]
    fn the_reset_strategy_takes_kafkas_three_forms() {
        assert_eq!(parse_reset("earliest"), Some(Reset::Earliest));
        assert_eq!(parse_reset("LATEST"), Some(Reset::Latest));
        assert_eq!(parse_reset("by_duration:PT1H"), Some(Reset::ByDuration(3_600_000)));
        assert_eq!(parse_reset("by_duration:P1DT30M"), Some(Reset::ByDuration(86_400_000 + 1_800_000)));
        assert_eq!(parse_reset("by_duration:PT0.5S"), Some(Reset::ByDuration(500)));
        for bad in ["none", "by_duration:", "by_duration:PT-1H", "by_duration:1H", "by_duration:PT"] {
            assert_eq!(parse_reset(bad), None, "{bad}");
        }
        assert!(refused(&[set(RESET, "none")]).1.starts_with("Invalid value `none` for configuration share.auto.offset.reset."));
    }

    #[test]
    fn a_session_must_be_longer_than_the_heartbeat() {
        let r = apply(
            "g",
            &BTreeMap::new(),
            &[set(SESSION, "45000"), set(HEARTBEAT, "15000")],
            false,
        );
        assert!(r.is_ok());
        let settings = ShareSettings::of(&r.unwrap());
        assert_eq!((settings.session_ms, settings.heartbeat_ms), (45_000, 15_000));
        assert_eq!(ShareSettings::of(&BTreeMap::new()), ShareSettings::default());
    }
}
