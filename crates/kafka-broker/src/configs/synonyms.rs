//! Where a topic config's value would come from if the topic did not set it: the broker config that stands behind it, in
//! the places Kafka looks (source: `ServerTopicConfigSynonyms`, `ConfigHelper.configSynonyms`).

use super::broker::{self, CHANGEABLE};
use super::live::Live;
use super::view::{DEFAULT, DYNAMIC_BROKER, DYNAMIC_DEFAULT, STATIC_BROKER};
use crate::topic_config;

/// The broker config with the highest priority behind a topic config.
fn broker_name(topic_key: &str) -> Option<&'static str> {
    Some(match topic_key {
        "segment.bytes" => "log.segment.bytes",
        "segment.ms" => "log.roll.ms",
        "segment.jitter.ms" => "log.roll.jitter.ms",
        "segment.index.bytes" => "log.index.size.max.bytes",
        "flush.messages" => "log.flush.interval.messages",
        "flush.ms" => "log.flush.interval.ms",
        "retention.bytes" => "log.retention.bytes",
        "retention.ms" => "log.retention.ms",
        "max.message.bytes" => "message.max.bytes",
        "index.interval.bytes" => "log.index.interval.bytes",
        "delete.retention.ms" => "log.cleaner.delete.retention.ms",
        "min.compaction.lag.ms" => "log.cleaner.min.compaction.lag.ms",
        "max.compaction.lag.ms" => "log.cleaner.max.compaction.lag.ms",
        "file.delete.delay.ms" => "log.segment.delete.delay.ms",
        "min.cleanable.dirty.ratio" => "log.cleaner.min.cleanable.ratio",
        "cleanup.policy" => "log.cleanup.policy",
        "preallocate" => "log.preallocate",
        "message.timestamp.type" => "log.message.timestamp.type",
        "message.timestamp.before.max.ms" => "log.message.timestamp.before.max.ms",
        "message.timestamp.after.max.ms" => "log.message.timestamp.after.max.ms",
        "local.retention.ms" => "log.local.retention.ms",
        "local.retention.bytes" => "log.local.retention.bytes",
        "unclean.leader.election.enable"
        | "min.insync.replicas"
        | "compression.type"
        | "compression.gzip.level"
        | "compression.lz4.level"
        | "compression.zstd.level" => return topic_config::lookup_name(topic_key),
        _ => return None,
    })
}

/// The places a topic config's value could come from other than the topic: name, value and source of each, the one
/// that would apply first first.
pub(crate) fn of_topic_key(live: &Live, topic_key: &str) -> Vec<(String, String, i8)> {
    let Some(name) = broker_name(topic_key) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let kafka_default = topic_config::kafka_default_of(topic_key).unwrap_or_default();
    // The default Kafka lists is the one of the broker config it is built from: retention and rolling are given in hours.
    let (default_name, default_value) = match topic_key {
        "retention.ms" => ("log.retention.hours", "168".to_owned()),
        "segment.ms" => ("log.roll.hours", "168".to_owned()),
        _ => (name, kafka_default.clone()),
    };
    if let Some(c) = CHANGEABLE.iter().find(|c| c.topic == topic_key) {
        let dynamic = live.dynamic();
        if let Some(v) = dynamic.own.get(c.name) {
            out.push((c.name.to_owned(), v.clone(), DYNAMIC_BROKER));
        }
        if let Some(v) = dynamic.cluster.get(c.name) {
            out.push((c.name.to_owned(), v.clone(), DYNAMIC_DEFAULT));
        }
        let started = broker::value_of(&live.started_with(), c.name).unwrap_or_default();
        if started != kafka_default {
            out.push((c.name.to_owned(), started, STATIC_BROKER));
        }
    }
    out.push((default_name.to_owned(), default_value, DEFAULT));
    out
}
