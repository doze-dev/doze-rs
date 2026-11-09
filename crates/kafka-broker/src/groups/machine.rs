//! One classic consumer group as a state machine with no clock, no tasks and no channels.
//!
//! [`GroupMachine::step`] takes an event and the time and returns what is to be done: answers to send and timers to
//! set. Whoever drives it (an actor per group) owns the channels and the clock; a test drives it with a number. A
//! request that cannot be answered yet (a JoinGroup waiting for the others, a SyncGroup waiting for the leader) is
//! held by its [`Token`], and answered by a later step.
//!
//! The states and their transitions are Kafka's: a group is `Empty`, `PreparingRebalance` (waiting for the members
//! to rejoin), `CompletingRebalance` (waiting for the leader's assignment) or `Stable`.

mod admin;
mod commit;
mod join;
mod leave;
mod rebalance;
mod sync;
mod timers;
mod types;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};

use bytes::Bytes;
use kafka_wire::ids::{InstanceId, MemberId};

pub(crate) use commit::commit_to_no_group;
pub(crate) use join::refusal;
pub(crate) use types::{
    Answer, CommitRequest, Description, Effect, Event, HeartbeatRequest, JoinAnswer, JoinMember,
    JoinRequest, LeaveResult, LeaveTarget, MemberDescription, Millis, Protocol, SyncAnswer,
    SyncRequest, TimerKey, TimerKind, Token,
};

/// The shortest and longest session a member may ask for (Kafka's `group.min.session.timeout.ms` and
/// `group.max.session.timeout.ms` defaults).
pub(crate) const SESSION_MS: std::ops::RangeInclusive<i32> = 6_000..=1_800_000;
/// A rebalance that no member gave a timeout for waits this long.
const DEFAULT_REBALANCE_MS: u64 = 30_000;

/// Where a group is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    /// No members.
    Empty,
    /// Waiting for the members to join again.
    PreparingRebalance,
    /// Joined; waiting for the leader to say who does what.
    CompletingRebalance,
    /// Running.
    Stable,
}

struct Member {
    id: MemberId,
    instance: Option<InstanceId>,
    client_id: String,
    session_ms: u64,
    rebalance_ms: u64,
    protocols: Vec<Protocol>,
    assignment: Bytes,
    /// A JoinGroup of this member that is waiting for the rebalance.
    joined: Option<Token>,
    /// A SyncGroup of this member that is waiting for the leader.
    syncing: Option<Token>,
    /// The arming of its session timer that counts.
    session_gen: u64,
}

/// A rebalance in progress.
struct Rebalance {
    /// When members that have not rejoined are dropped.
    deadline: Millis,
    generation: u64,
    /// The initial delay's window, if the group is still holding.
    hold: Option<Hold>,
}

struct Hold {
    /// A member that was not in the group joined during this window.
    new_member: bool,
    generation: u64,
}

/// Makes a member id; given the instance id of a static member, which the id carries.
pub(crate) type IdSource = Box<dyn FnMut(Option<&InstanceId>) -> MemberId + Send>;

/// A classic consumer group.
pub(crate) struct GroupMachine {
    initial_delay_ms: u64,
    new_id: IdSource,
    state: State,
    generation: i32,
    protocol_type: Option<String>,
    protocol: Option<String>,
    leader: Option<MemberId>,
    members: BTreeMap<MemberId, Member>,
    /// Static members: instance id to the member id it holds.
    statics: HashMap<InstanceId, MemberId>,
    rebalance: Option<Rebalance>,
    next_generation: u64,
}

impl GroupMachine {
    /// An empty group. `initial_delay_ms` is how long the first rebalance of an empty group waits for more members
    /// to arrive (zero: not at all); `new_id` makes member ids.
    #[must_use]
    pub(crate) fn new(initial_delay_ms: u64, new_id: IdSource) -> Self {
        Self {
            initial_delay_ms,
            new_id,
            state: State::Empty,
            generation: 0,
            protocol_type: None,
            protocol: None,
            leader: None,
            members: BTreeMap::new(),
            statics: HashMap::new(),
            rebalance: None,
            next_generation: 0,
        }
    }

    /// Handles one event at time `now`.
    pub(crate) fn step(&mut self, event: Event, now: Millis) -> Vec<Effect> {
        let mut out = Vec::new();
        match event {
            Event::Join(token, request) => self.join(token, &request, now, &mut out),
            Event::Sync(token, request) => self.sync(token, request, now, &mut out),
            Event::Heartbeat(token, request) => self.heartbeat(token, &request, now, &mut out),
            Event::Leave(token, targets) => self.leave(token, &targets, now, &mut out),
            Event::Commit(token, request) => self.commit(token, &request, &mut out),
            Event::Describe(token) => self.describe(token, &mut out),
            Event::Delete(token) => self.delete(token, &mut out),
            Event::Timer(key) => self.timer(&key, now, &mut out),
            // A consumer group's: the owner of the group hands these to the right machine.
            Event::ConsumerHeartbeat(..) | Event::TopicsChanged => {}
        }
        out
    }
}

/// What the tests ask of a group.
#[cfg(test)]
impl GroupMachine {
    /// Where the group is.
    #[must_use]
    pub(crate) fn state(&self) -> State {
        self.state
    }

    /// The generation: it goes up with every rebalance that completes.
    #[must_use]
    pub(crate) fn generation(&self) -> i32 {
        self.generation
    }

    /// The members' ids, in order.
    #[must_use]
    pub(crate) fn member_ids(&self) -> Vec<MemberId> {
        self.members.keys().cloned().collect()
    }

    /// The leader, once there is one.
    #[must_use]
    pub(crate) fn leader(&self) -> Option<&MemberId> {
        self.leader.as_ref()
    }

    /// The protocol chosen for this generation.
    #[must_use]
    pub(crate) fn protocol(&self) -> Option<&str> {
        self.protocol.as_deref()
    }
}

impl GroupMachine {
    /// How many members there are.
    #[must_use]
    pub(crate) fn member_count(&self) -> usize {
        self.members.len()
    }

    /// The protocol type the members share.
    #[must_use]
    pub(crate) fn protocol_type(&self) -> Option<&str> {
        self.protocol_type.as_deref()
    }

    fn next_arming(&mut self) -> u64 {
        self.next_generation += 1;
        self.next_generation
    }

    /// Starts (or restarts) a member's session timer.
    fn arm_session(&mut self, id: &MemberId, now: Millis, out: &mut Vec<Effect>) {
        let generation = self.next_arming();
        if let Some(m) = self.members.get_mut(id) {
            m.session_gen = generation;
            out.push(Effect::Arm {
                key: TimerKey {
                    kind: TimerKind::Session(id.clone()),
                    generation,
                },
                at: now.plus(m.session_ms),
            });
        }
    }

    /// The members in the form the leader is told of them, under the protocol chosen.
    fn join_members(&self) -> Vec<JoinMember> {
        let chosen = self.protocol.as_deref();
        self.members
            .values()
            .map(|m| JoinMember {
                id: m.id.clone(),
                instance: m.instance.clone(),
                metadata: m
                    .protocols
                    .iter()
                    .find(|p| Some(p.name.as_str()) == chosen)
                    .map(|p| p.metadata.clone())
                    .unwrap_or_default(),
            })
            .collect()
    }

    /// What a member is told when it joins the generation now running: the leader also gets the members.
    fn join_answer(&self, id: &MemberId) -> JoinAnswer {
        let leads = self.leader.as_ref() == Some(id);
        JoinAnswer {
            error: None,
            generation: self.generation,
            protocol_type: self.protocol_type.clone(),
            protocol: self.protocol.clone(),
            leader: self.leader.clone(),
            member_id: Some(id.clone()),
            members: if leads {
                self.join_members()
            } else {
                Vec::new()
            },
        }
    }

    /// The member `id` names, if it is one and any static instance named fits it. `Err` is what to answer.
    fn fenced(
        &self,
        id: &MemberId,
        instance: Option<&InstanceId>,
    ) -> Result<(), kafka_wire::ResponseError> {
        use kafka_wire::ResponseError as E;
        if let Some(inst) = instance {
            match self.statics.get(inst) {
                None => return Err(E::UnknownMemberId),
                Some(held) if held != id => return Err(E::FencedInstanceId),
                Some(_) => {}
            }
        }
        if self.members.contains_key(id) {
            Ok(())
        } else {
            Err(E::UnknownMemberId)
        }
    }
}
