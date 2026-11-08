//! SyncGroup (the leader's assignment, handed to everyone) and Heartbeat.

use bytes::Bytes;
use kafka_wire::ResponseError;

use super::{
    Answer, Effect, GroupMachine, HeartbeatRequest, Millis, State, SyncAnswer, SyncRequest, Token,
};

impl GroupMachine {
    pub(super) fn sync(
        &mut self,
        token: Token,
        req: SyncRequest,
        now: Millis,
        out: &mut Vec<Effect>,
    ) {
        let refuse = |out: &mut Vec<Effect>, e| {
            out.push(Effect::Reply(token, Answer::Sync(SyncAnswer::refused(e))));
        };
        // Kafka's order: who, then which generation, then which protocol, then the group's state.
        if let Err(e) = self.fenced(&req.member_id, req.instance.as_ref()) {
            return refuse(out, e);
        }
        if req.generation != self.generation {
            return refuse(out, ResponseError::IllegalGeneration);
        }
        let wrong = |asked: &Option<String>, has: &Option<String>| asked.is_some() && asked != has;
        if wrong(&req.protocol_type, &self.protocol_type) || wrong(&req.protocol, &self.protocol) {
            return refuse(out, ResponseError::InconsistentGroupProtocol);
        }
        match self.state {
            State::Empty => refuse(out, ResponseError::UnknownMemberId),
            State::PreparingRebalance => {
                self.arm_session(&req.member_id, now, out);
                refuse(out, ResponseError::RebalanceInProgress);
            }
            State::CompletingRebalance => self.await_assignment(token, req, now, out),
            State::Stable => {
                self.arm_session(&req.member_id, now, out);
                let assignment = self
                    .members
                    .get(&req.member_id)
                    .map(|m| m.assignment.clone())
                    .unwrap_or_default();
                out.push(Effect::Reply(token, Answer::Sync(self.synced(assignment))));
            }
        }
    }

    fn synced(&self, assignment: Bytes) -> SyncAnswer {
        SyncAnswer {
            error: None,
            protocol_type: self.protocol_type.clone(),
            protocol: self.protocol.clone(),
            assignment,
        }
    }

    /// The group is waiting for its leader's assignment. The leader's SyncGroup brings it and answers everyone; a
    /// member's other SyncGroup waits for it.
    fn await_assignment(
        &mut self,
        token: Token,
        req: SyncRequest,
        now: Millis,
        out: &mut Vec<Effect>,
    ) {
        self.arm_session(&req.member_id, now, out);
        if self.leader.as_ref() != Some(&req.member_id) {
            if let Some(m) = self.members.get_mut(&req.member_id) {
                // A second SyncGroup replaces the first, which is told its member is not known.
                if let Some(first) = m.syncing.replace(token) {
                    out.push(Effect::Reply(
                        first,
                        Answer::Sync(SyncAnswer::refused(ResponseError::UnknownMemberId)),
                    ));
                }
            }
            return;
        }
        for m in self.members.values_mut() {
            m.assignment = Bytes::new();
        }
        for (id, assignment) in req.assignments {
            if let Some(m) = self.members.get_mut(&id) {
                m.assignment = assignment;
            }
        }
        self.state = State::Stable;
        let ids: Vec<_> = self.members.keys().cloned().collect();
        for id in ids {
            let Some(m) = self.members.get_mut(&id) else {
                continue;
            };
            let assignment = m.assignment.clone();
            if let Some(waiting) = m.syncing.take() {
                out.push(Effect::Reply(
                    waiting,
                    Answer::Sync(self.synced(assignment)),
                ));
            } else if id == req.member_id {
                out.push(Effect::Reply(token, Answer::Sync(self.synced(assignment))));
            }
            self.arm_session(&id, now, out);
        }
    }

    pub(super) fn heartbeat(
        &mut self,
        token: Token,
        req: &HeartbeatRequest,
        now: Millis,
        out: &mut Vec<Effect>,
    ) {
        let answer = if let Err(e) = self.fenced(&req.member_id, req.instance.as_ref()) {
            Some(e)
        } else if req.generation != self.generation {
            Some(ResponseError::IllegalGeneration)
        } else {
            self.arm_session(&req.member_id, now, out);
            (self.state == State::PreparingRebalance).then_some(ResponseError::RebalanceInProgress)
        };
        out.push(Effect::Reply(token, Answer::Heartbeat(answer)));
    }
}
