//! What DescribeConfigs says of a broker: the configs it has, each with where its value comes from and the places it
//! could have come from (source: `ConfigHelper.createBrokerConfigEntry`, `configSynonyms`; the layout of the recorded
//! answers is `group-coordinator-defaults`).

use std::collections::BTreeMap;

use super::broker::{self, CHANGEABLE};
use super::live::Live;
use crate::Config;

/// Where a value comes from, as DescribeConfigs numbers it.
pub(crate) const DYNAMIC_BROKER: i8 = 2;
pub(crate) const DYNAMIC_DEFAULT: i8 = 3;
pub(crate) const STATIC_BROKER: i8 = 4;
pub(crate) const DEFAULT: i8 = 5;

const BOOLEAN: i8 = 1;
const INT: i8 = 3;
const LONG: i8 = 5;

/// One config of a broker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct View {
    pub(crate) name: &'static str,
    pub(crate) value: String,
    pub(crate) kind: i8,
    pub(crate) read_only: bool,
    /// Where the value in force comes from.
    pub(crate) source: i8,
    /// Every place a value was given, the one in force first.
    pub(crate) synonyms: Vec<(i8, String)>,
}

fn millis(d: std::time::Duration) -> String {
    d.as_millis().to_string()
}

/// A config the broker holds and does not change while it runs: its value, and Kafka's default for it.
struct Fixed {
    name: &'static str,
    kind: i8,
    value: String,
    kafka_default: &'static str,
}

fn fixed(config: &Config) -> Vec<Fixed> {
    let f = |name, kind, value: String, kafka_default| Fixed {
        name,
        kind,
        value,
        kafka_default,
    };
    vec![
        f(
            "group.consumer.heartbeat.interval.ms",
            INT,
            "5000".into(),
            "5000",
        ),
        f(
            "group.consumer.min.assignment.interval.ms",
            INT,
            "0".into(),
            "0",
        ),
        f(
            "offsets.retention.check.interval.ms",
            LONG,
            config.offsets_check_every.map_or("600000".into(), millis),
            "600000",
        ),
        f(
            "group.consumer.session.timeout.ms",
            INT,
            millis(config.consumer_session_timeout),
            "45000",
        ),
        f(
            "offsets.retention.minutes",
            INT,
            (config.offsets_retention.as_secs() / 60).to_string(),
            "10080",
        ),
        f(
            "group.initial.rebalance.delay.ms",
            INT,
            millis(config.group_initial_rebalance_delay),
            "3000",
        ),
        f(
            "num.partitions",
            INT,
            config.default_partitions.to_string(),
            "1",
        ),
        f(
            "auto.create.topics.enable",
            BOOLEAN,
            config.auto_create_topics.to_string(),
            "true",
        ),
        f("broker.id", INT, config.node_id.to_string(), "-1"),
        f("node.id", INT, config.node_id.to_string(), "-1"),
        f(
            "log.retention.check.interval.ms",
            LONG,
            millis(config.log_retention_check_every),
            "300000",
        ),
        f(
            "transactional.id.expiration.ms",
            INT,
            millis(config.transactional_id_expiration),
            "604800000",
        ),
    ]
}

fn view_of_fixed(f: Fixed) -> View {
    let mut synonyms = Vec::new();
    let source = if f.value == f.kafka_default {
        DEFAULT
    } else {
        synonyms.push((STATIC_BROKER, f.value.clone()));
        STATIC_BROKER
    };
    synonyms.push((DEFAULT, f.kafka_default.to_owned()));
    View {
        name: f.name,
        value: f.value,
        kind: f.kind,
        read_only: f.name != "group.consumer.assignment.interval.ms",
        source,
        synonyms,
    }
}

/// The configs of this broker, as it has them now.
pub(crate) fn of_broker(config: &Config, live: &Live) -> Vec<View> {
    let now = live.get();
    let started = live.started_with();
    let dynamic = live.dynamic();
    let kafka = broker::kafka_defaults();
    let mut out = vec![view_of_fixed(Fixed {
        name: "group.consumer.assignment.interval.ms",
        kind: INT,
        value: millis(config.consumer_assignment_interval),
        kafka_default: "1000",
    })];
    out.extend(fixed(config).into_iter().map(view_of_fixed));
    for c in &CHANGEABLE {
        let place = |d: &BTreeMap<String, String>| d.get(c.name).cloned();
        let mut synonyms = Vec::new();
        if let Some(v) = place(&dynamic.own) {
            synonyms.push((DYNAMIC_BROKER, v));
        }
        if let Some(v) = place(&dynamic.cluster) {
            synonyms.push((DYNAMIC_DEFAULT, v));
        }
        let start = broker::value_of(&started, c.name).unwrap_or_default();
        let kafka_default = broker::value_of(&kafka, c.name).unwrap_or_default();
        if start != kafka_default {
            synonyms.push((STATIC_BROKER, start));
        }
        synonyms.push((DEFAULT, kafka_default));
        out.push(View {
            name: c.name,
            value: broker::value_of(&now, c.name).unwrap_or_default(),
            kind: c.kind,
            read_only: false,
            source: synonyms.first().map_or(DEFAULT, |s| s.0),
            synonyms,
        });
    }
    out
}

/// What all brokers share: the configs set for every broker, and only those.
pub(crate) fn of_cluster(live: &Live) -> Vec<View> {
    let dynamic = live.dynamic();
    CHANGEABLE
        .iter()
        .filter_map(|c| {
            let value = dynamic.cluster.get(c.name)?.clone();
            Some(View {
                name: c.name,
                value: value.clone(),
                kind: c.kind,
                read_only: false,
                source: DYNAMIC_DEFAULT,
                synonyms: vec![(DYNAMIC_DEFAULT, value)],
            })
        })
        .collect()
}
