//! Classic members in a consumer group (a rolling upgrade, in either direction). A classic member is told what to do by
//! the classic APIs and kept in step with the group: its JoinGroup is answered with no leader and no member list (nobody
//! relays assignments, the group computes them), its SyncGroup carries the partitions it may have now, and its
//! Heartbeat says REBALANCE_IN_PROGRESS when only a rejoin can move its partitions.
//!
//! What a classic member's JoinGroup says it still owns is what it holds: an eager consumer owns nothing by then, which
//! is what lets another member take its partitions; a cooperative one owns what it had, and has given up the rest when
//! a later JoinGroup no longer lists it.

use kafka_wire::ResponseError as E;
use kafka_wire::ids::{InstanceId, MemberId};

mod calls;
mod join;

use super::assign::{Assignor, Partitions};
use super::member::{ClassicSeat, Member};
use super::protocol::{
    ByName, Subscription, decode_assignment, decode_subscription, encode_assignment,
};
use super::{Config, ConsumerGroup, Topics};
use crate::groups::machine::{Effect, IdSource, Millis};
use crate::groups::seat::{Handover, Seat};

/// Partitions as classic consumers name them.
pub(super) fn partitions_of(by_name: &ByName, topics: &Topics) -> Partitions {
    by_name
        .iter()
        .filter_map(|(name, ps)| {
            topics
                .id_of(name)
                .map(|id| ps.iter().map(move |p| (id, *p)))
        })
        .flatten()
        .collect()
}

pub(super) fn names_of(parts: &Partitions, topics: &Topics) -> ByName {
    let mut out = ByName::new();
    for (id, p) in parts {
        if let Some(name) = topics.name_of(id) {
            out.entry(name.to_owned()).or_default().push(*p);
        }
    }
    out
}

/// The first of the protocols offered whose subscription can be read, and the name of the protocol.
pub(super) fn subscription(
    protocols: &[crate::groups::machine::Protocol],
) -> Option<(Subscription, String)> {
    protocols
        .iter()
        .find_map(|p| decode_subscription(&p.metadata).map(|s| (s, p.name.clone())))
}

impl ConsumerGroup {
    /// A consumer group made of the members of a classic group, each keeping the partitions it held, which no other
    /// member may take until it has rejoined and given them up.
    pub(crate) fn adopt(
        cfg: Config,
        ids: IdSource,
        handover: Handover,
        topics: &Topics,
        now: Millis,
        out: &mut Vec<Effect>,
    ) -> Self {
        let epoch = handover.generation;
        let mut group = Self::restored(cfg, ids, epoch, epoch, Assignor::Uniform);
        for seat in handover.seats {
            let Some((sub, protocol)) = subscription(&seat.protocols) else {
                continue;
            };
            let held = partitions_of(
                &decode_assignment(&seat.assignment).unwrap_or_default(),
                topics,
            );
            let id = seat.id.clone();
            let mut m = Member::new(seat.id);
            m.instance = seat.instance;
            m.client_id = seat.client_id;
            m.client_host = seat.client_host;
            m.subscribed = sub.topics;
            m.epoch = epoch;
            m.prev_epoch = epoch;
            m.target = held.clone();
            m.assigned = held;
            m.rebalance_ms = i32::try_from(seat.rebalance_ms).unwrap_or(i32::MAX);
            // What it held was the classic leader's to say; a topic its principal may not DESCRIBE is not held (SEC-234).
            m.rights = seat.rights;
            m.forget_hidden(topics);
            m.classic = Some(ClassicSeat {
                protocols: seat.protocols,
                protocol,
                version: sub.version,
                session_ms: seat.session_ms,
            });
            group.members.insert(id.clone(), m);
            group.reset_session(&id, now, out);
        }
        group
    }

    /// Whether the last member of the consumer protocol has gone, leaving classic members only.
    pub(crate) fn take_downgrade(&mut self) -> bool {
        std::mem::take(&mut self.check_downgrade)
    }

    /// The members as a classic group's, if all of them are classic: the group is a classic group again, and each must
    /// rejoin.
    pub(crate) fn hand_back(&mut self, topics: &Topics, out: &mut Vec<Effect>) -> Option<Handover> {
        if self.members.is_empty() || self.members.values().any(|m| m.classic.is_none()) {
            return None;
        }
        let mut seats = Vec::with_capacity(self.members.len());
        for m in self.members.values() {
            Self::disarm(&m.id, out);
            let Some(seat) = &m.classic else { continue };
            seats.push(Seat {
                id: m.id.clone(),
                instance: m.instance.clone(),
                client_id: m.client_id.clone(),
                client_host: m.client_host.clone(),
                protocols: seat.protocols.clone(),
                assignment: encode_assignment(seat.version, &names_of(&m.assigned, topics)),
                session_ms: seat.session_ms,
                rebalance_ms: u64::try_from(m.rebalance_ms)
                    .ok()
                    .filter(|ms| *ms > 0)
                    .unwrap_or(seat.session_ms),
                rights: m.rights.clone(),
            });
        }
        Some(Handover {
            generation: self.epoch,
            seats,
        })
    }

    /// What a request naming a static instance is answered when the instance is not the member's.
    pub(super) fn static_code(
        &self,
        member: &MemberId,
        instance: Option<&InstanceId>,
    ) -> Option<E> {
        self.static_refusal(Some(member), instance)
            .err()
            .and_then(|a| a.error)
            .map(|(e, _)| e)
    }

    pub(super) fn must_rejoin(&self, id: &MemberId) -> bool {
        let Some(m) = self.members.get(id) else {
            return false;
        };
        m.assigned.iter().any(|p| !m.target.contains(p))
            || m.target
                .iter()
                .any(|p| !m.assigned.contains(p) && self.free(id, p))
    }

    /// Starts the rebalance timeout of every classic member that holds a partition it is no longer to have: such a
    /// member only gives it up by rejoining, and one that never does would keep it from its new owner for ever.
    pub(super) fn arm_classic_revocations(&mut self, now: Millis, out: &mut Vec<Effect>) {
        let due: Vec<MemberId> = self
            .members
            .values()
            .filter(|m| m.classic.is_some() && m.revoke_gen == 0)
            .filter(|m| m.assigned.iter().any(|p| !m.target.contains(p)))
            .map(|m| m.id.clone())
            .collect();
        for id in due {
            self.arm_revoke(&id, now, out);
        }
    }
}
