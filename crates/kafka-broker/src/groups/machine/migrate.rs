//! A classic group handing its members to a consumer group (a consumer-protocol member has joined it), and being made
//! again from the members of a consumer group that has no consumer-protocol member left.

use std::collections::BTreeMap;

use bytes::Bytes;
use kafka_wire::ResponseError;
use kafka_wire::ids::MemberId;

use super::{
    Answer, Effect, GroupMachine, IdSource, JoinAnswer, Member, Millis, State, SyncAnswer,
    TimerKind,
};
use crate::groups::consumer::protocol::decode_subscription;
use crate::groups::note::{Cause, Why};
use crate::groups::seat::{Handover, Seat};

impl GroupMachine {
    /// Hands the members over, if this can be a consumer group: its protocol type is `consumer` and every member offers
    /// a subscription that can be read (the new coordinator reads the topics from it). What is parked is answered
    /// REBALANCE_IN_PROGRESS, a JoinGroup and a SyncGroup alike, with the member's own id: the rebalance it waited on is
    /// over, and the member asks again with the id it has and finds itself a member of the consumer group. The group is
    /// left with its timers disarmed, to be dropped.
    pub(crate) fn hand_over(&mut self, out: &mut Vec<Effect>) -> Option<Handover> {
        if self.protocol_type.as_deref() != Some("consumer") || !self.members.values().all(readable)
        {
            return None;
        }
        out.push(Effect::Disarm(TimerKind::Rebalance));
        out.push(Effect::Disarm(TimerKind::Hold));
        let mut seats = Vec::with_capacity(self.members.len());
        for m in self.members.values_mut() {
            out.push(Effect::Disarm(TimerKind::Session(m.id.clone())));
            if let Some(token) = m.joined.take() {
                let refused =
                    JoinAnswer::refused(ResponseError::RebalanceInProgress, Some(m.id.clone()));
                out.push(Effect::Reply(token, Answer::Join(refused)));
            }
            if let Some(token) = m.syncing.take() {
                out.push(Effect::Reply(
                    token,
                    Answer::Sync(SyncAnswer::refused(ResponseError::RebalanceInProgress)),
                ));
            }
            // Between the members' JoinGroups and the leader's SyncGroup a generation has no assignments yet, and a
            // member still has what the one before gave it: a cooperative consumer is reading it.
            let held = if self.state == State::CompletingRebalance {
                m.previous.clone()
            } else {
                m.assignment.clone()
            };
            seats.push(Seat {
                id: m.id.clone(),
                instance: m.instance.clone(),
                client_id: m.client_id.clone(),
                client_host: m.client_host.clone(),
                protocols: m.protocols.clone(),
                assignment: held,
                session_ms: m.session_ms,
                rebalance_ms: m.rebalance_ms,
                rights: m.rights.clone(),
            });
        }
        Some(Handover {
            generation: self.generation,
            seats,
        })
    }

    /// A classic group made of these members, none of whom holds a generation of this group: every one must join
    /// again, and the next heartbeat of each is told its generation is gone.
    pub(crate) fn from_handover(
        initial_delay_ms: u64,
        ids: IdSource,
        handover: Handover,
        now: Millis,
        out: &mut Vec<Effect>,
    ) -> Self {
        let mut group = Self::new(initial_delay_ms, ids);
        group.generation = handover.generation.saturating_add(1);
        group.protocol_type = Some("consumer".to_owned());
        let mut members = BTreeMap::new();
        for seat in handover.seats {
            if let Some(instance) = &seat.instance {
                group.statics.insert(instance.clone(), seat.id.clone());
            }
            members.insert(
                seat.id.clone(),
                Member {
                    id: seat.id,
                    instance: seat.instance,
                    client_id: seat.client_id,
                    client_host: seat.client_host,
                    session_ms: seat.session_ms,
                    rebalance_ms: seat.rebalance_ms,
                    protocols: seat.protocols,
                    assignment: seat.assignment,
                    previous: Bytes::new(),
                    joined: None,
                    syncing: None,
                    session_gen: 0,
                    heard: Millis(0),
                    rights: seat.rights,
                },
            );
        }
        group.members = members;
        let ids: Vec<MemberId> = group.members.keys().cloned().collect();
        for id in &ids {
            group.arm_session(id, now, out);
        }
        // Begun from a running state, so that it starts the wait for the members to join again.
        group.state = State::Stable;
        let why = Why::new(Cause::BecameClassic, None, group.members.len());
        group.begin_rebalance(now, false, why, out);
        group
    }
}

/// Whether the member offers a protocol whose subscription can be read.
fn readable(m: &Member) -> bool {
    m.protocols
        .iter()
        .any(|p| decode_subscription(&p.metadata).is_some())
}
