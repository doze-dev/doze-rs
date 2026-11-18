//! A streams group (KIP-1071, the streams rebalance protocol): Kafka Streams instances heartbeat with their topology, and the
//! broker works out the tasks it comes to, makes the internal topics it needs, and gives each member its active, standby
//! and warm-up tasks, revoking before it assigns as a consumer group of the new protocol does.
//!
//! The rules are Kafka 4.4.0's `GroupMetadataManager.streamsGroupHeartbeat` and its helpers
//! (docs/port/behaviours/streams-groups.md). Pure, like the other kinds: the actor hands in the time and the topics; the
//! internal topics to make, and the member's timers, go out as the answer and effects say.

mod assign;
mod endpoints;
mod fence;
mod heartbeat;
mod member;
mod reconcile;
mod seat;
mod steps;
pub(crate) mod stored;
mod tasks;
#[cfg(test)]
mod tests;
pub(crate) mod topology;
mod view;

use std::collections::BTreeMap;

use kafka_wire::ResponseError as E;
use kafka_wire::Uuid;
use kafka_wire::ids::{InstanceId, MemberId};

pub(crate) use self::endpoints::EndpointPartitions;
pub(crate) use self::fence::fetch_refusal;
use self::member::Member;
pub(crate) use self::reconcile::Stage;
pub(crate) use self::tasks::{Task, Tasks, by_subtopology, from_wire};
use self::topology::{Configured, Creatable, Topology};
pub(crate) use self::view::{StreamsMemberView, StreamsView};
use super::machine::{Effect, Millis, TimerKey, TimerKind};

/// What a streams group runs by: its configs, or the broker's defaults, as the heartbeat finds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StreamsTiming {
    /// `streams.session.timeout.ms`.
    pub(crate) session_ms: u64,
    /// `streams.heartbeat.interval.ms`.
    pub(crate) heartbeat_ms: i32,
    /// `streams.initial.rebalance.delay.ms`.
    pub(crate) initial_delay_ms: u64,
    /// `streams.assignment.interval.ms`.
    pub(crate) assign_interval_ms: u64,
    /// `streams.task.offset.interval.ms`.
    pub(crate) task_offset_ms: i32,
    /// `streams.acceptable.recovery.lag`.
    pub(crate) acceptable_lag: i64,
    /// `streams.num.standby.replicas`.
    pub(crate) standbys: usize,
    /// `streams.rack.aware.assignment.tags`.
    pub(crate) tags: Vec<String>,
    /// `group.streams.max.size`.
    pub(crate) max_size: usize,
}

/// An endpoint for interactive queries.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Endpoint {
    pub(crate) host: String,
    pub(crate) port: u16,
}

/// A StreamsGroupHeartbeat, after the checks that need nothing but the request.
#[derive(Debug, Clone)]
pub(crate) struct StreamsHeartbeat {
    pub(crate) group: String,
    pub(crate) member_id: MemberId,
    /// 0 to join, -1 to leave, -2 for a static member to leave and come back, else the epoch the member is at.
    pub(crate) epoch: i32,
    /// The request's version: some statuses are for version 1 clients only.
    pub(crate) version: i16,
    pub(crate) endpoint_epoch: i32,
    pub(crate) instance: Option<InstanceId>,
    pub(crate) rack: Option<String>,
    /// -1: unchanged.
    pub(crate) rebalance_ms: i32,
    /// Only when joining.
    pub(crate) topology: Option<Topology>,
    /// What it owns, when it says (all three lists, or none).
    pub(crate) owned: Option<Tasks>,
    pub(crate) process: Option<String>,
    pub(crate) endpoint: Option<Endpoint>,
    pub(crate) client_tags: Option<BTreeMap<String, String>>,
    pub(crate) offsets: Option<Vec<(Task, i64)>>,
    pub(crate) end_offsets: Option<Vec<(Task, i64)>>,
    pub(crate) shutdown: bool,
    pub(crate) client_id: String,
    pub(crate) client_host: String,
    pub(crate) timing: StreamsTiming,
}

/// The answer to a StreamsGroupHeartbeat.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct StreamsAnswer {
    pub(crate) error: Option<(E, String)>,
    pub(crate) member_id: Option<MemberId>,
    pub(crate) epoch: i32,
    pub(crate) heartbeat_ms: i32,
    pub(crate) task_offset_ms: i32,
    pub(crate) acceptable_lag: i64,
    /// Codes and words, in Kafka's order.
    pub(crate) status: Vec<(i8, String)>,
    /// What the member is to hold, when it joined or that changed.
    pub(crate) tasks: Option<Tasks>,
    pub(crate) endpoint_epoch: i32,
    /// Where every member's tasks are, when the member's view of it is out of date.
    pub(crate) endpoints: Option<Vec<EndpointPartitions>>,
    /// The internal topics to be made (the handler checks the caller may, and makes them without waiting).
    pub(crate) create: Vec<Creatable>,
}

impl StreamsAnswer {
    pub(crate) fn refused(error: E, words: impl Into<String>) -> Self {
        Self {
            error: Some((error, words.into())),
            ..Self::default()
        }
    }
}

/// The status codes of the heartbeat answer that are not about the topics (KIP-1071).
pub(crate) mod status {
    pub(crate) const STALE_TOPOLOGY: i8 = 0;
    pub(crate) const SHUTDOWN_APPLICATION: i8 = 4;
    pub(crate) const ASSIGNMENT_DELAYED: i8 = 5;
    pub(crate) const MISSING_CLIENT_TAGS: i8 = 6;
}

/// A streams group.
pub(crate) struct StreamsGroup {
    epoch: i32,
    assignment_epoch: i32,
    /// When the targets were last computed; `None` (Kafka's timestamp 0): at the next chance, whatever the interval.
    assigned_at: Option<Millis>,
    topology: Option<Topology>,
    /// The topology against the topics, as last worked out; `None` until a heartbeat works it out.
    configured: Option<Configured>,
    /// The topics the configuration was worked out against (Kafka's metadata hash).
    seen: BTreeMap<String, (Uuid, i32)>,
    /// The topology's epoch when it was last ready, else -1.
    validated_epoch: i32,
    /// The assignment configs the targets were last computed with: standby replicas and tags.
    assignment_configs: Option<(usize, Vec<String>)>,
    members: BTreeMap<MemberId, Member>,
    targets: BTreeMap<MemberId, Tasks>,
    /// Moves whenever a member's endpoint or tasks change (memory only, as in Kafka).
    endpoint_epoch: i32,
    /// The member that asked for the whole application to stop, until the group is empty.
    shutdown_by: Option<MemberId>,
    /// The initial rebalance delay is running.
    delaying: bool,
    /// `streams.assignment.interval.ms`, as the last heartbeat found it.
    assign_interval_ms: u64,
    next_arming: u64,
}

/// What a streams group keeps across a restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Kept {
    pub(crate) epoch: i32,
    pub(crate) assignment_epoch: i32,
    pub(crate) topology: Option<Topology>,
}

pub(super) fn not_a_member(member: &str, group: &str) -> String {
    format!("Member {member} is not a member of group {group}.")
}

impl StreamsGroup {
    /// A group nobody has joined: at epoch 1, its targets too (Kafka's `StreamsGroup` and `TargetAssignmentMetadata.INITIAL`),
    /// so the first member's join takes it to 2 and a member waiting out the initial rebalance delay is at 1.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::restored(Kept {
            epoch: 1,
            assignment_epoch: 1,
            topology: None,
        })
    }

    /// An empty group as it was when the broker last stopped.
    #[must_use]
    pub(crate) fn restored(kept: Kept) -> Self {
        Self {
            epoch: kept.epoch,
            assignment_epoch: kept.assignment_epoch,
            assigned_at: None,
            topology: kept.topology,
            configured: None,
            seen: BTreeMap::new(),
            validated_epoch: -1,
            assignment_configs: None,
            members: BTreeMap::new(),
            targets: BTreeMap::new(),
            endpoint_epoch: 0,
            shutdown_by: None,
            delaying: false,
            assign_interval_ms: 0,
            next_arming: 0,
        }
    }

    #[must_use]
    pub(crate) fn member_count(&self) -> usize {
        self.members.len()
    }

    /// What is kept across a restart; `with` is a topology a join is about to set, for a group that has none yet.
    #[must_use]
    pub(crate) fn kept(&self, with: Option<&Topology>) -> Kept {
        Kept {
            epoch: self.epoch,
            assignment_epoch: self.assignment_epoch,
            topology: self.topology.clone().or_else(|| with.cloned()),
        }
    }

    /// Whether this heartbeat would seat a member who is not in the group now.
    #[must_use]
    pub(crate) fn would_seat(&self, hb: &StreamsHeartbeat) -> bool {
        hb.epoch == 0 && !self.members.contains_key(&hb.member_id) && self.has_room(hb)
    }

    fn has_room(&self, hb: &StreamsHeartbeat) -> bool {
        let rejoining = hb
            .instance
            .as_ref()
            .is_some_and(|i| self.members.values().any(|m| m.instance.as_ref() == Some(i)));
        rejoining || self.members.len() < hb.timing.max_size
    }

    /// The topics the topology reads, for OffsetDelete: none while nobody is in the group (Kafka's `isSubscribedToTopic`).
    #[must_use]
    pub(crate) fn subscribed(&self) -> std::collections::BTreeSet<String> {
        if self.members.is_empty() {
            return std::collections::BTreeSet::new();
        }
        self.topology
            .iter()
            .flat_map(|t| t.subtopologies.values())
            .flat_map(|s| s.inputs().map(str::to_owned))
            .collect()
    }

    fn arm(&mut self, kind: TimerKind, at: Millis, out: &mut Vec<Effect>) -> u64 {
        self.next_arming += 1;
        let generation = self.next_arming;
        out.push(Effect::Arm {
            key: TimerKey { kind, generation },
            at,
        });
        generation
    }
}
