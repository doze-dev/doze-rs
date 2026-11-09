//! The broker's own configs: the few that can be changed while it runs, and the way Kafka judges a change to them
//! (source: `DynamicBrokerConfig`, `ConfigAdminManager.validateBrokerConfigChange`).

use std::collections::BTreeMap;

use super::Refusal;
use super::live::Dynamic;
use crate::Defaults;
use kafka_wire::ResponseError as E;

/// A config that can be changed, and the smallest value it takes.
pub(crate) struct Changeable {
    pub(crate) name: &'static str,
    pub(crate) least: i64,
    /// The largest value it takes (32-bit configs).
    pub(crate) most: i64,
    /// Kafka's type number for it.
    pub(crate) kind: i8,
    /// The topic config it is the default of.
    pub(crate) topic: &'static str,
}

const INT: i8 = 3;
const LONG: i8 = 5;

/// The configs that act on a running broker here: the defaults that topics lay their own over.
pub(crate) const CHANGEABLE: [Changeable; 5] = [
    Changeable {
        name: "log.segment.bytes",
        least: 1_048_576,
        most: i32::MAX as i64,
        kind: INT,
        topic: "segment.bytes",
    },
    Changeable {
        name: "log.index.interval.bytes",
        least: 0,
        most: i32::MAX as i64,
        kind: INT,
        topic: "index.interval.bytes",
    },
    Changeable {
        name: "message.max.bytes",
        least: 0,
        most: i32::MAX as i64,
        kind: INT,
        topic: "max.message.bytes",
    },
    Changeable {
        name: "log.retention.bytes",
        least: i64::MIN,
        most: i64::MAX,
        kind: LONG,
        topic: "retention.bytes",
    },
    Changeable {
        name: "log.retention.ms",
        least: -1,
        most: i64::MAX,
        kind: LONG,
        topic: "retention.ms",
    },
];

/// Broker configs that Kafka has and this broker does not change while it runs (the rest it does not have).
const FIXED: [&str; 14] = [
    "advertised.listeners",
    "auto.create.topics.enable",
    "broker.id",
    "group.initial.rebalance.delay.ms",
    "listeners",
    "log.dirs",
    "log.retention.check.interval.ms",
    "node.id",
    "num.partitions",
    "offsets.retention.minutes",
    "process.roles",
    "transactional.id.expiration.ms",
    "controller.quorum.voters",
    "controller.listener.names",
];

pub(crate) fn changeable(name: &str) -> Option<&'static Changeable> {
    CHANGEABLE.iter().find(|c| c.name == name)
}

/// The default `name` makes, as text.
pub(crate) fn value_of(defaults: &Defaults, name: &str) -> Option<String> {
    Some(
        match name {
            "log.segment.bytes" => defaults.segment_bytes,
            "log.index.interval.bytes" => defaults.index_interval_bytes,
            "message.max.bytes" => defaults.message_max_bytes,
            "log.retention.bytes" => defaults.retention_bytes,
            "log.retention.ms" => defaults.retention_ms,
            _ => return None,
        }
        .to_string(),
    )
}

/// The defaults with the dynamic configs laid over them, the broker's own over the cluster's.
pub(crate) fn overlay(base: Defaults, dynamic: &Dynamic) -> Defaults {
    let mut out = base;
    for set in [&dynamic.cluster, &dynamic.own] {
        for (name, value) in set {
            // Read as `validate` reads it: a number may have spaces around it (Kafka's `ConfigDef.parseType` trims).
            let Ok(n) = value.trim().parse::<i64>() else {
                continue;
            };
            match name.as_str() {
                "log.segment.bytes" => out.segment_bytes = n,
                "log.index.interval.bytes" => out.index_interval_bytes = n,
                "message.max.bytes" => out.message_max_bytes = n,
                "log.retention.bytes" => out.retention_bytes = n,
                "log.retention.ms" => out.retention_ms = n,
                _ => {}
            }
        }
    }
    out
}

fn refuse(message: String) -> Refusal {
    Refusal {
        code: E::InvalidRequest,
        message,
    }
}

/// Whether the broker configs may be as given: Kafka's checks in its order. Every refusal is INVALID_REQUEST.
///
/// # Errors
/// A config that is Kafka's but cannot be changed while it runs, one that is not Kafka's, or a value that is not
/// one the config takes.
pub(crate) fn validate(configs: &BTreeMap<String, String>) -> Result<(), Refusal> {
    let fixed: Vec<&str> = configs
        .keys()
        .map(String::as_str)
        .filter(|n| FIXED.contains(n))
        .collect();
    if !fixed.is_empty() {
        return Err(refuse(format!(
            "Cannot update these configs dynamically: [{}]",
            fixed.join(", ")
        )));
    }
    for (name, value) in configs {
        let Some(c) = changeable(name) else {
            return Err(refuse(format!("Unknown Dynamic Configuration: {name}.")));
        };
        let wrong = |why: &str| {
            refuse(format!(
                "Invalid value {value} for configuration {name}: {why}"
            ))
        };
        let n: i64 = value.trim().parse().map_err(|_| {
            wrong(if c.kind == INT {
                "Not a number of type INT"
            } else {
                "Not a number of type LONG"
            })
        })?;
        if n < c.least && c.least != i64::MIN {
            return Err(wrong(&format!("Value must be at least {}", c.least)));
        }
        if n > c.most {
            return Err(wrong("Not a number of type INT"));
        }
    }
    Ok(())
}

/// What Kafka's own defaults are for the configs here.
pub(crate) fn kafka_defaults() -> Defaults {
    Defaults {
        segment_bytes: 1 << 30,
        index_interval_bytes: 4096,
        message_max_bytes: 1_048_588,
        retention_bytes: -1,
        retention_ms: 7 * 24 * 3_600_000,
    }
}
