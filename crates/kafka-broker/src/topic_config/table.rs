//! The topic configs, as Kafka names them. Defaults are what Kafka reports (recorded: `topic-config-keys`),
//! except where this broker's differ and say so.

use crate::Defaults;

/// What a value of a config looks like.
#[derive(Debug, Clone, Copy)]
pub(super) enum Kind {
    /// A 32-bit number of at least `min`.
    Int { min: i64 },
    /// A 64-bit number of at least `min`.
    Long { min: i64 },
    /// A decimal between `min` and `max`.
    Double { min: f64, max: f64 },
    /// `true` or `false`.
    Bool,
    /// One of these.
    Choice(&'static [&'static str]),
    /// `delete`, `compact` or a list of them.
    CleanupPolicy,
    /// Anything.
    Text,
}

/// A config's value when a topic does not set it.
#[derive(Debug, Clone, Copy)]
pub(super) enum Fallback {
    /// Not reported.
    None,
    /// This value.
    Fixed(&'static str),
    /// The broker's own setting, and what Kafka's own default for it is (a different value is a static broker config).
    Broker(fn(&Defaults) -> String, &'static str),
}

#[derive(Debug)]
pub(super) struct Setting {
    pub(super) name: &'static str,
    pub(super) kind: Kind,
    pub(super) default: Fallback,
}

use Fallback::{Broker, Fixed};
use Kind::{Bool, Choice, CleanupPolicy, Double, Int, Long, Text};

const fn s(name: &'static str, kind: Kind, default: Fallback) -> Setting {
    Setting {
        name,
        kind,
        default,
    }
}

const NO_LIMIT: &str = "9223372036854775807";
const TRUE_FALSE: Kind = Bool;

/// Kafka's way of saying "no bound": -1, where this broker's defaults use it too.
fn bound(v: i64) -> String {
    v.max(-1).to_string()
}

/// Every topic config, by name.
pub(super) static SETTINGS: &[Setting] = &[
    s("cleanup.policy", CleanupPolicy, Fixed("delete")),
    s(
        "compression.type",
        Choice(&["uncompressed", "zstd", "lz4", "snappy", "gzip", "producer"]),
        Fixed("producer"),
    ),
    s("compression.gzip.level", Int { min: -1 }, Fixed("-1")),
    s("compression.lz4.level", Int { min: 1 }, Fixed("9")),
    s("compression.zstd.level", Int { min: -131_072 }, Fixed("3")),
    s("delete.retention.ms", Long { min: 0 }, Fixed("86400000")),
    s("file.delete.delay.ms", Long { min: 0 }, Fixed("60000")),
    s("flush.messages", Long { min: 1 }, Fixed(NO_LIMIT)),
    s("flush.ms", Long { min: 0 }, Fixed(NO_LIMIT)),
    s("follower.replication.throttled.replicas", Text, Fixed("")),
    s(
        "index.interval.bytes",
        Int { min: 0 },
        Broker(|d| d.index_interval_bytes.to_string(), "4096"),
    ),
    s("leader.replication.throttled.replicas", Text, Fixed("")),
    s("local.retention.bytes", Long { min: -2 }, Fixed("-2")),
    s("local.retention.ms", Long { min: -2 }, Fixed("-2")),
    s("max.compaction.lag.ms", Long { min: 1 }, Fixed(NO_LIMIT)),
    s(
        "max.message.bytes",
        Int { min: 0 },
        Broker(|d| d.message_max_bytes.to_string(), "1048588"),
    ),
    s("message.downconversion.enable", TRUE_FALSE, Fallback::None),
    s("message.format.version", Text, Fallback::None),
    s(
        "message.timestamp.after.max.ms",
        Long { min: 0 },
        Fixed("3600000"),
    ),
    s(
        "message.timestamp.before.max.ms",
        Long { min: 0 },
        Fixed(NO_LIMIT),
    ),
    s(
        "message.timestamp.difference.max.ms",
        Long { min: 0 },
        Fallback::None,
    ),
    s(
        "message.timestamp.type",
        Choice(&["CreateTime", "LogAppendTime"]),
        Fixed("CreateTime"),
    ),
    s(
        "min.cleanable.dirty.ratio",
        Double { min: 0.0, max: 1.0 },
        Fixed("0.5"),
    ),
    s("min.compaction.lag.ms", Long { min: 0 }, Fixed("0")),
    s("min.insync.replicas", Int { min: 1 }, Fixed("1")),
    s("preallocate", TRUE_FALSE, Fixed("false")),
    s("remote.log.copy.disable", TRUE_FALSE, Fixed("false")),
    s("remote.log.delete.on.disable", TRUE_FALSE, Fixed("false")),
    s("remote.storage.enable", TRUE_FALSE, Fixed("false")),
    s(
        "retention.bytes",
        Long { min: -1 },
        Broker(|d| bound(d.retention_bytes), "-1"),
    ),
    s(
        "retention.ms",
        Long { min: -1 },
        Broker(|d| bound(d.retention_ms), "604800000"),
    ),
    // Kafka asks for a megabyte since 4.0. Lower here, so retention can be tried on a handful of records, but
    // not so low that every record is a file of its own: a segment is two open files while its partition is in use.
    s(
        "segment.bytes",
        Int { min: 1024 },
        Broker(|d| d.segment_bytes.to_string(), "1073741824"),
    ),
    s("segment.index.bytes", Int { min: 4 }, Fixed("10485760")),
    s("segment.jitter.ms", Long { min: 0 }, Fixed("0")),
    s("segment.ms", Long { min: 1 }, Fixed("604800000")),
    s("unclean.leader.election.enable", TRUE_FALSE, Fixed("false")),
];

impl Kind {
    /// What a value may be, as a person says it (the Config page of the console teaches from this).
    #[cfg_attr(not(feature = "inspect"), allow(dead_code))]
    pub(super) fn allowed(&self) -> String {
        match self {
            Self::Int { min } | Self::Long { min } => format!("a whole number, at least {min}"),
            Self::Double { min, max } => format!("a number from {min} to {max}"),
            Self::Bool => "true or false".to_owned(),
            Self::Choice(all) => format!("one of {}", all.join(", ")),
            Self::CleanupPolicy => "compact or delete, each element checked".to_owned(),
            Self::Text => "any text".to_owned(),
        }
    }
}
