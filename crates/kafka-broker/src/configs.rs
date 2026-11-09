//! Changing a topic's configuration: the operations of AlterConfigs and IncrementalAlterConfigs reduced to the map of
//! overrides the topic is to have, checked as Apache Kafka 4.3.1's controller does (source:
//! `ConfigurationControlManager`, `ConfigAdminManager`).

use std::collections::{BTreeMap, BTreeSet};

use kafka_wire::ResponseError as E;

use crate::{Defaults, topic_config};

pub(crate) mod broker;
mod live;
pub(crate) mod synonyms;
pub(crate) mod view;
pub(crate) use live::{Dynamic, Live};

/// One change to one config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Op {
    /// Set it to the value.
    Set,
    /// Remove the topic's own value, so that the default applies.
    Delete,
    /// Add the values to a list.
    Append,
    /// Take the values out of a list.
    Subtract,
}

impl Op {
    /// The operation by the number IncrementalAlterConfigs gives it.
    #[must_use]
    pub(crate) fn from_wire(n: i8) -> Option<Self> {
        match n {
            0 => Some(Self::Set),
            1 => Some(Self::Delete),
            2 => Some(Self::Append),
            3 => Some(Self::Subtract),
            _ => None,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Set => "SET",
            Self::Delete => "DELETE",
            Self::Append => "APPEND",
            Self::Subtract => "SUBTRACT",
        }
    }
}

/// A change asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Change {
    pub(crate) name: String,
    pub(crate) op: Op,
    /// `None`: the request gave no value.
    pub(crate) value: Option<String>,
}

/// Why a change is refused, as the answer gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub(crate) code: E,
    pub(crate) message: String,
}

fn refuse(code: E, message: impl Into<String>) -> Refusal {
    Refusal {
        code,
        message: message.into(),
    }
}

/// The longest value Kafka keeps.
const LONGEST_VALUE: usize = 32_767;

/// What the topic's own configs are after the changes, or why not. `current` is the configs it has now (none, for a topic
/// that is not there: the configs are judged first and the topic's absence after, as Kafka does).
///
/// # Errors
/// The first refusal, in Kafka's order: a config named twice or given no value (INVALID_REQUEST); an append to or subtraction
/// from something that is not a list, an unknown or badly valued config, a value too long, or a minimum compaction lag above
/// the maximum (INVALID_CONFIG).
pub(crate) fn apply(
    defaults: &Defaults,
    current: &BTreeMap<String, String>,
    changes: &[Change],
) -> Result<BTreeMap<String, String>, Refusal> {
    let next = prepare(current, changes, topic_config::is_list, |name| {
        topic_config::default_of(defaults, name)
    })?;
    if sets_too_long_a_value(changes, &next) {
        return Err(refuse(
            E::InvalidConfig,
            format!(
                "The configuration value cannot be added because it exceeds the maximum value size of {LONGEST_VALUE} bytes."
            ),
        ));
    }
    topic_config::validate(&next).map_err(|m| refuse(E::InvalidConfig, m))?;
    Ok(next)
}

/// Whether a value the request sets is too long. Kafka looks only at the values a request changes, and counts them as Java
/// counts a string, in UTF-16 units (source: `ConfigurationControlManager`, `configRecord.value().length()`).
fn sets_too_long_a_value(changes: &[Change], next: &BTreeMap<String, String>) -> bool {
    changes
        .iter()
        .filter(|c| c.op != Op::Delete)
        .filter_map(|c| next.get(&c.name))
        .any(|v| v.encode_utf16().count() > LONGEST_VALUE)
}

/// The configs of a broker after the changes: those it can change while it runs, judged as Kafka judges them.
///
/// # Errors
/// As [`apply`] for the changes themselves, then [`broker::validate`].
pub(crate) fn apply_to_broker(
    current: &BTreeMap<String, String>,
    changes: &[Change],
) -> Result<BTreeMap<String, String>, Refusal> {
    let next = prepare(current, changes, |_| false, |_| None)?;
    broker::validate(&next)?;
    Ok(next)
}

/// The changes laid over the current configs: the checks that come before any config is judged on its own.
fn prepare(
    current: &BTreeMap<String, String>,
    changes: &[Change],
    is_list: impl Fn(&str) -> bool,
    default_of: impl Fn(&str) -> Option<String>,
) -> Result<BTreeMap<String, String>, Refusal> {
    check_request(changes)?;
    let mut next = current.clone();
    for c in changes {
        let value = c.value.clone().unwrap_or_default();
        match c.op {
            Op::Set => {
                next.insert(c.name.clone(), value);
            }
            Op::Delete => {
                next.remove(&c.name);
            }
            Op::Append | Op::Subtract => {
                if !is_list(&c.name) {
                    return Err(refuse(
                        E::InvalidConfig,
                        format!(
                            "Can't {} to key {} because its type is not LIST.",
                            c.op.name(),
                            c.name
                        ),
                    ));
                }
                let base = next
                    .get(&c.name)
                    .cloned()
                    .or_else(|| default_of(&c.name))
                    .unwrap_or_default();
                next.insert(c.name.clone(), change_list(&base, &value, &c.op));
            }
        }
    }
    Ok(next)
}

/// The checks Kafka makes of a resource's changes before looking at what they are for: no config named twice, and a
/// value for each that is not a deletion.
///
/// # Errors
/// INVALID_REQUEST.
pub(crate) fn check_request(changes: &[Change]) -> Result<(), Refusal> {
    let mut seen = BTreeSet::new();
    if changes.iter().any(|c| !seen.insert(c.name.as_str())) {
        return Err(refuse(
            E::InvalidRequest,
            "Error due to duplicate config keys",
        ));
    }
    let missing: Vec<&str> = changes
        .iter()
        .filter(|c| c.value.is_none() && c.op != Op::Delete)
        .map(|c| c.name.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(refuse(
            E::InvalidRequest,
            format!("Null value not supported for : {}", missing.join(", ")),
        ));
    }
    Ok(())
}

/// A comma-separated list with the values added (those not there) or taken out.
fn change_list(base: &str, values: &str, op: &Op) -> String {
    let mut list: Vec<&str> = base.split(',').filter(|v| !v.is_empty()).collect();
    for v in values.split(',').filter(|v| !v.is_empty()) {
        match op {
            Op::Append if !list.contains(&v) => list.push(v),
            Op::Subtract => list.retain(|x| *x != v),
            _ => {}
        }
    }
    list.join(",")
}

#[cfg(test)]
mod tests;
