//! A classic member's SyncGroup, Heartbeat and LeaveGroup to a consumer group.

use kafka_wire::ResponseError as E;

use super::super::ConsumerGroup;
use super::super::Topics;
use super::names_of;
use crate::groups::consumer::protocol::encode_assignment;
use crate::groups::machine::{
    Answer, Effect, HeartbeatRequest, LeaveResult, LeaveTarget, Millis, SyncAnswer, SyncRequest,
    Token,
};

impl ConsumerGroup {
    pub(in crate::groups::consumer) fn classic_sync(
        &mut self,
        token: Token,
        req: &SyncRequest,
        topics: &Topics,
        now: Millis,
        out: &mut Vec<Effect>,
    ) {
        let answer = self.sync_classic(req, topics, now, out);
        out.push(Effect::Reply(token, Answer::Sync(answer)));
    }

    /// Nobody uploads assignments: the group has computed them, and the member is given the partitions it may have now.
    fn sync_classic(
        &mut self,
        req: &SyncRequest,
        topics: &Topics,
        now: Millis,
        out: &mut Vec<Effect>,
    ) -> SyncAnswer {
        if let Some(e) = self.static_code(&req.member_id, req.instance.as_ref()) {
            return SyncAnswer::refused(e);
        }
        let target_epoch = self.target_epoch;
        let Some(m) = self
            .members
            .get_mut(&req.member_id)
            .filter(|m| m.classic.is_some())
        else {
            return SyncAnswer::refused(E::UnknownMemberId);
        };
        let Some(seat) = m.classic.as_ref() else {
            return SyncAnswer::refused(E::UnknownMemberId);
        };
        if req.generation != m.epoch {
            return SyncAnswer::refused(E::IllegalGeneration);
        }
        if req
            .protocol_type
            .as_deref()
            .is_some_and(|t| t != "consumer")
            || req.protocol.as_deref().is_some_and(|p| p != seat.protocol)
        {
            return SyncAnswer::refused(E::InconsistentGroupProtocol);
        }
        // The group has moved on since this member joined, and it is not in the middle of giving partitions up: there
        // is nothing for it to collect, it must join again.
        if m.epoch != target_epoch && m.revoking.is_empty() {
            return SyncAnswer::refused(E::RebalanceInProgress);
        }
        m.sent = Some(m.assigned.clone());
        let answer = SyncAnswer {
            error: None,
            protocol_type: Some("consumer".to_owned()),
            protocol: Some(seat.protocol.clone()),
            assignment: encode_assignment(seat.version, &names_of(&m.assigned, topics)),
        };
        let id = req.member_id.clone();
        self.reset_session(&id, now, out);
        answer
    }

    pub(in crate::groups::consumer) fn classic_heartbeat(
        &mut self,
        token: Token,
        req: &HeartbeatRequest,
        topics: &Topics,
        now: Millis,
        out: &mut Vec<Effect>,
    ) {
        let error = self.heartbeat_classic(req, topics, now, out);
        out.push(Effect::Reply(token, Answer::Heartbeat(error)));
    }

    /// Keeps a classic member's session, and says whether it must rejoin: its generation is behind the group's, or what
    /// it holds is not what it is to have and something can move now. The rights of the heartbeat's caller are the
    /// member's from now on, and a topic they no longer let through makes it rejoin (SEC-231).
    fn heartbeat_classic(
        &mut self,
        req: &HeartbeatRequest,
        topics: &Topics,
        now: Millis,
        out: &mut Vec<Effect>,
    ) -> Option<E> {
        if let Some(e) = self.static_code(&req.member_id, req.instance.as_ref()) {
            return Some(e);
        }
        let Some(m) = self
            .members
            .get(&req.member_id)
            .filter(|m| m.classic.is_some())
        else {
            return Some(E::UnknownMemberId);
        };
        if req.generation != m.epoch {
            return Some(E::IllegalGeneration);
        }
        if self.refresh_rights(&req.member_id, req.rights.clone(), topics, now, out) {
            self.bump_and_reassign(topics, now);
        }
        let behind = self
            .members
            .get(&req.member_id)
            .is_some_and(|m| m.epoch != self.target_epoch);
        self.reset_session(&req.member_id, now, out);
        let rejoin = behind || self.must_rejoin(&req.member_id);
        // Told to rejoin to give something up, it has until its rebalance timeout to do it.
        self.arm_classic_revocations(now, out);
        rejoin.then_some(E::RebalanceInProgress)
    }

    /// A LeaveGroup removes the members it names, a consumer-protocol one too (as an admin tool removes one). The group is
    /// not made classic again by this, even if the one named was the last of the consumer protocol.
    pub(in crate::groups::consumer) fn classic_leave(
        &mut self,
        token: Token,
        targets: &[LeaveTarget],
        out: &mut Vec<Effect>,
    ) {
        let mut removed = false;
        let mut results = Vec::with_capacity(targets.len());
        for t in targets {
            let found = match (&t.member_id, &t.instance) {
                (Some(id), _) => self.members.contains_key(id).then(|| id.clone()),
                (None, Some(instance)) => self.holder_of(instance).cloned(),
                (None, None) => None,
            };
            let error = match found {
                Some(id) => {
                    self.remove_member(&id, out);
                    removed = true;
                    None
                }
                None => Some(E::UnknownMemberId),
            };
            results.push(LeaveResult {
                member_id: t.member_id.clone(),
                instance: t.instance.clone(),
                error,
            });
        }
        if removed {
            self.bump_for_removal();
        }
        out.push(Effect::Reply(token, Answer::Leave(results)));
    }
}
