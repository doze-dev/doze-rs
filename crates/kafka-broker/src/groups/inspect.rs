//! What the console is told of a group that DescribeGroups and ConsumerGroupDescribe do not say (feature `inspect`).
//!
//! It rides on the group's [`Description`](super::machine::Description), so it is read in the same turn as the rest: one
//! consistent look at one group. Every time is relative to the moment the group was asked, on the group's own clock, so
//! the console never needs to know that clock.

use kafka_wire::ids::MemberId;

/// A rebalance in progress (classic groups).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RebalanceInspect {
    /// How long ago it began.
    pub(crate) began_ago_ms: u64,
    /// How long members have left to join again.
    pub(crate) remaining_ms: u64,
    /// The wait it was given: the longest rebalance timeout among the members when it began.
    pub(crate) timeout_ms: u64,
}

/// One member's timeouts and how lately it was heard from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MemberInspect {
    pub(crate) id: MemberId,
    /// The session timeout it asked for (a consumer-protocol member: the group's).
    pub(crate) session_ms: u64,
    /// The rebalance timeout it asked for; `None` if it did not say.
    pub(crate) rebalance_ms: Option<u64>,
    /// How long ago its session timer was last started: a join, a sync or a heartbeat of its own.
    pub(crate) heard_ago_ms: u64,
    /// It has joined again in the rebalance that is running (always true when none is).
    pub(crate) rejoined: bool,
}

/// The hidden facts of one group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Inspect {
    /// A classic group's generation, a consumer group's epoch.
    pub(crate) generation: i32,
    /// The classic leader, once there is one.
    pub(crate) leader: Option<MemberId>,
    /// The rebalance running now.
    pub(crate) rebalance: Option<RebalanceInspect>,
    pub(crate) members: Vec<MemberInspect>,
}
