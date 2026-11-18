//! The group configs kept, in Kafka 4.4.0's order of definition (`GroupConfig.CONFIG_DEF`): the share group ones, then the
//! streams group ones.

use super::streams::{
    ACCEPTABLE_LAG, ASSIGNOR, INITIAL_DELAY, OFFLOAD, STANDBYS, STREAMS_ASSIGN, STREAMS_HEARTBEAT,
    STREAMS_SESSION, TAGS, TASK_OFFSETS, WARMUPS,
};
use super::{ASSIGN, DELIVERY, HEARTBEAT, ISOLATION, LOCK, LOCKS, RENEW, RESET, SESSION};

/// The kind of value a config takes, as Kafka's `ConfigDef` types it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Int { at_least: i64 },
    Long { at_least: i64 },
    Text,
    Bool,
    /// A comma-separated list.
    List,
}

impl Kind {
    /// Kafka's `ConfigType` code, as DescribeConfigs reports it.
    #[must_use]
    pub(crate) fn code(self) -> i8 {
        match self {
            Self::Bool => 1,
            Self::Text => 2,
            Self::Int { .. } => 3,
            Self::Long { .. } => 5,
            Self::List => 7,
        }
    }
}

/// One group config: its name, kind, Kafka's default as text, and its broker synonym.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Def {
    pub(crate) name: &'static str,
    pub(crate) kind: Kind,
    pub(crate) default: &'static str,
    pub(crate) synonym: Option<&'static str>,
    /// Kafka defines it as internal: DescribeConfigs lists it only where the group sets it.
    pub(crate) internal: bool,
}

const fn def(
    name: &'static str,
    kind: Kind,
    default: &'static str,
    synonym: Option<&'static str>,
) -> Def {
    Def {
        name,
        kind,
        default,
        synonym,
        internal: false,
    }
}

const fn internal(name: &'static str, kind: Kind, default: &'static str, synonym: &'static str) -> Def {
    Def {
        name,
        kind,
        default,
        synonym: Some(synonym),
        internal: true,
    }
}

const fn int(at_least: i64) -> Kind {
    Kind::Int { at_least }
}

/// The group configs kept.
pub(crate) const DEFS: &[Def] = &[
    def(SESSION, int(1), "45000", Some("group.share.session.timeout.ms")),
    def(HEARTBEAT, int(1), "5000", Some("group.share.heartbeat.interval.ms")),
    def(LOCK, int(1000), "30000", Some("group.share.record.lock.duration.ms")),
    def(DELIVERY, int(2), "5", Some("group.share.delivery.count.limit")),
    def(LOCKS, int(100), "2000", Some("group.share.partition.max.record.locks")),
    def(RESET, Kind::Text, "latest", None),
    def(ISOLATION, Kind::Text, "read_uncommitted", None),
    def(RENEW, Kind::Bool, "true", None),
    def(ASSIGN, int(0), "1000", Some("group.share.assignment.interval.ms")),
    def(STREAMS_SESSION, int(1), "45000", Some("group.streams.session.timeout.ms")),
    def(STREAMS_HEARTBEAT, int(1), "5000", Some("group.streams.heartbeat.interval.ms")),
    def(STANDBYS, int(0), "0", Some("group.streams.num.standby.replicas")),
    def(INITIAL_DELAY, int(0), "3000", Some("group.streams.initial.rebalance.delay.ms")),
    def(STREAMS_ASSIGN, int(0), "1000", Some("group.streams.assignment.interval.ms")),
    internal(OFFLOAD, Kind::Bool, "true", "group.streams.assignor.offload.enable"),
    internal(TASK_OFFSETS, int(1), "60000", "group.streams.task.offset.interval.ms"),
    internal(WARMUPS, int(0), "2", "group.streams.num.warmup.replicas"),
    def(TAGS, Kind::List, "", Some("group.streams.rack.aware.assignment.tags")),
    internal(ACCEPTABLE_LAG, Kind::Long { at_least: 0 }, "10000", "group.streams.acceptable.recovery.lag"),
    // Kafka reports the first registered assignor as its default, under the broker's list of them.
    def(ASSIGNOR, Kind::Text, "sticky", Some("group.streams.assignors")),
];
