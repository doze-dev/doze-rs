//! A consumer group of the new protocol (KIP-848): the broker decides who owns which partition, and members only
//! heartbeat.
//!
//! The group is a state machine with no clock, no tasks and no channels, like the classic one: the actor that owns it
//! hands in events, the time and a snapshot of the topics, and does what the effects say.
//!
//! **Who owns a partition** is the group's own record, never inferred from what a client reports. A member has
//! `assigned` (what it may consume now) and `revoking` (what it has been told to give up and has not said it has). A
//! partition is free for another member only when it is in neither set of any other member. A member says it has
//! given up a revocation by reporting, in a later heartbeat, owned partitions that include none of them; until then
//! (or until its rebalance timeout ends its membership) they stay its own.
//!
//! **When targets are computed.** The group has an `epoch`, bumped when membership, subscriptions or topics change, and
//! a `target_epoch`: the epoch the members' targets were last computed for. Below the epoch the group is `Assigning`.
//! Targets are computed by the first heartbeat (or classic JoinGroup) at least the assignment interval after the last
//! computation, never by a leave, an expiry or the passing of time. That is what Kafka does and what is recorded.

pub(crate) mod assign;
mod classic;
mod fetch;
mod heartbeat;
mod join;
mod member;
mod pattern;
pub(crate) mod protocol;
mod reconcile;
mod rights;
mod step;
mod timers;
mod types;
mod view;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use kafka_wire::ids::MemberId;

use self::assign::Assignor;
pub(crate) use self::fetch::fetch_refusal;
pub(crate) use self::join::{MOST_MEMBERS, can_join, join_refusal};
#[allow(unused_imports)]
pub(crate) use self::member::ClassicSeat;
use self::member::Member;
pub(crate) use self::types::{
    ConsumerView, Heartbeat, HeartbeatAnswer, MemberView, TopicInfo, TopicSource, Topics,
};
use super::machine::{Effect, IdSource, Millis, TimerKey, TimerKind};

/// How a consumer group is run.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Config {
    /// How long a member may be silent.
    pub(crate) session_ms: u64,
    /// The least time between two computations of the targets (Kafka's `group.consumer.assignment.interval.ms`).
    pub(crate) assign_interval_ms: u64,
}

/// A consumer group.
pub(crate) struct ConsumerGroup {
    cfg: Config,
    new_id: IdSource,
    epoch: i32,
    target_epoch: i32,
    /// When the targets were last computed.
    assigned_at: Option<Millis>,
    assignor: Assignor,
    members: BTreeMap<MemberId, Member>,
    next_arming: u64,
    /// A consumer-protocol member has gone: if only classic members are left, the group is classic again.
    check_downgrade: bool,
}

impl ConsumerGroup {
    /// A group nobody has joined. Its epoch is 1, so that the first member's join takes it to 2, the first epoch Kafka
    /// gives a member of a new group.
    #[must_use]
    pub(crate) fn new(cfg: Config, new_id: IdSource) -> Self {
        Self::restored(cfg, new_id, 1, 1, Assignor::Uniform)
    }

    /// An empty group as it was when the broker last stopped.
    #[must_use]
    pub(crate) fn restored(
        cfg: Config,
        new_id: IdSource,
        epoch: i32,
        target_epoch: i32,
        assignor: Assignor,
    ) -> Self {
        Self {
            cfg,
            new_id,
            epoch,
            target_epoch,
            assigned_at: None,
            assignor,
            members: BTreeMap::new(),
            next_arming: 0,
            check_downgrade: false,
        }
    }

    /// How many members there are.
    #[must_use]
    pub(crate) fn member_count(&self) -> usize {
        self.members.len()
    }

    /// The epochs and assignor, for the record kept across a restart.
    pub(crate) fn epochs(&self) -> (i32, i32, Assignor) {
        (self.epoch, self.target_epoch, self.assignor)
    }

    fn arming(&mut self) -> u64 {
        self.next_arming += 1;
        self.next_arming
    }

    /// The timer of this kind, to run out at `at`.
    fn arm(&mut self, kind: TimerKind, at: Millis, out: &mut Vec<Effect>) -> u64 {
        let generation = self.arming();
        out.push(Effect::Arm {
            key: TimerKey { kind, generation },
            at,
        });
        generation
    }
}
