//! ConsumerGroupHeartbeat: one request that joins, keeps on and leaves. Kafka's words are the recorded ones.

use kafka_wire::ResponseError as E;
use kafka_wire::ids::{InstanceId, MemberId};

use super::assign::Partitions;
use super::join::{Parsed, parse};
use super::pattern::Pattern;
use super::{ConsumerGroup, Heartbeat, HeartbeatAnswer, Topics};
use crate::groups::machine::{Answer, Effect, Millis, Token};

pub(super) fn not_a_member(member: &str, group: &str) -> String {
    format!("Member {member} is not a member of group {group}.")
}

pub(super) fn stale_epoch(sent: i32, known: i32) -> String {
    let than = if sent < known { "smaller" } else { "greater" };
    format!(
        "The consumer group member has a {than} member epoch ({sent}) than the one known by the group coordinator \
         ({known}). The member must abandon all its partitions and rejoin."
    )
}

fn text(id: Option<&MemberId>) -> &str {
    id.map_or("", MemberId::as_str)
}

impl ConsumerGroup {
    /// Handles one ConsumerGroupHeartbeat.
    pub(crate) fn heartbeat(
        &mut self,
        token: Token,
        req: &Heartbeat,
        topics: &Topics,
        now: Millis,
        out: &mut Vec<Effect>,
    ) {
        let answer = if req.epoch < 0 {
            self.leave(req, now, out)
        } else {
            self.join_or_poll(req, topics, now, out)
        };
        // The targets may have moved: a classic member that holds what it is no longer to have has until its rebalance
        // timeout to join again.
        self.arm_classic_revocations(now, out);
        out.push(Effect::Reply(token, Answer::ConsumerHeartbeat(answer)));
    }

    pub(super) fn holder_of(&self, instance: &InstanceId) -> Option<&MemberId> {
        self.members
            .values()
            .find(|m| m.instance.as_ref() == Some(instance))
            .map(|m| &m.id)
    }

    /// A request naming a static instance that is not the member's: nobody holds it, or another member does.
    pub(super) fn static_refusal(
        &self,
        member: Option<&MemberId>,
        instance: Option<&InstanceId>,
    ) -> Result<(), HeartbeatAnswer> {
        let Some(instance) = instance else {
            return Ok(());
        };
        match self.holder_of(instance) {
            None => Err(HeartbeatAnswer::refused(
                E::UnknownMemberId,
                format!("Instance id {instance} is unknown."),
            )),
            Some(holder) if Some(holder) != member => Err(HeartbeatAnswer::refused(
                E::FencedInstanceId,
                format!(
                    "Static member {} with instance id {instance} was fenced by member {holder}.",
                    text(member)
                ),
            )),
            Some(_) => Ok(()),
        }
    }

    fn done(id: &MemberId, epoch: i32) -> HeartbeatAnswer {
        HeartbeatAnswer {
            error: None,
            member_id: Some(id.clone()),
            epoch,
            assignment: None,
        }
    }

    /// Epoch -1 leaves; -2 is a static member going away to come back, which keeps its place and its partitions until
    /// its session runs out.
    fn leave(&mut self, req: &Heartbeat, now: Millis, out: &mut Vec<Effect>) -> HeartbeatAnswer {
        let member = req.member_id.as_ref();
        if let Err(refused) = self.static_refusal(member, req.instance.as_ref()) {
            return refused;
        }
        let Some(id) = member.filter(|id| self.members.contains_key(*id)).cloned() else {
            return HeartbeatAnswer::refused(
                E::UnknownMemberId,
                not_a_member(text(member), &req.group),
            );
        };
        if req.epoch == -2 && req.instance.is_some() {
            if let Some(m) = self.members.get_mut(&id) {
                m.left = true;
                m.revoke_gen = 0;
            }
            out.push(Effect::Disarm(crate::groups::machine::TimerKind::Revoke(
                id.clone(),
            )));
            self.reset_session(&id, now, out);
            return Self::done(&id, req.epoch);
        }
        self.remove_member(&id, out);
        self.bump_for_removal();
        self.check_downgrade = true;
        Self::done(&id, req.epoch)
    }

    fn join_or_poll(
        &mut self,
        req: &Heartbeat,
        topics: &Topics,
        now: Millis,
        out: &mut Vec<Effect>,
    ) -> HeartbeatAnswer {
        let joining = req.epoch == 0;
        let id = match (&req.member_id, joining) {
            (Some(id), _) => id.clone(),
            // A version 0 client sends no id and expects one; from version 1 the client makes its own.
            (None, true) if req.version == 0 => (self.new_id)(req.instance.as_ref()),
            (None, true) => {
                return HeartbeatAnswer::refused(E::InvalidRequest, "a member id is required");
            }
            (None, false) => {
                return HeartbeatAnswer::refused(E::UnknownMemberId, not_a_member("", &req.group));
            }
        };
        let Parsed { assignor, pattern } = match parse(req) {
            Ok(parsed) => parsed,
            Err(refused) => return refused,
        };
        if !joining && let Err(refused) = self.static_refusal(Some(&id), req.instance.as_ref()) {
            return refused;
        }
        if let Some(answer) = self.classic_beat(&id, req, now, out) {
            return answer;
        }
        let mut changed = false;
        let seated = if joining {
            self.seat(&id, req, &mut changed, out)
        } else {
            self.poll_check(&id, req)
        };
        if let Err(refused) = seated {
            return refused;
        }
        if let Some(m) = self.members.get_mut(&id) {
            m.client_id.clone_from(&req.client_id);
            m.client_host.clone_from(&req.client_host);
            m.instance.clone_from(&req.instance);
            m.rack.clone_from(&req.rack);
            if let Some(names) = &req.subscribed
                && m.subscribed != *names
            {
                m.subscribed.clone_from(names);
                changed = true;
            }
            if let Some(text) = &req.regex
                && text != m.pattern.as_ref().map_or("", Pattern::text)
            {
                m.pattern = pattern;
                changed = true;
            }
            if req.assignor.is_some() && assignor != m.assignor {
                m.assignor = assignor;
                changed = true;
            }
            // A right to DESCRIBE given or taken since the last heartbeat changes what the pattern takes.
            changed |= m.take_rights(req.rights.clone(), topics);
        }
        self.reset_session(&id, now, out);
        if changed {
            self.bump_and_reassign(topics, now);
        } else {
            self.assign_if_due(topics, now);
        }
        // `None`: the member did not say what it owns this time.
        let reported: Option<Partitions> = req.owned.as_ref().map(|o| {
            o.iter()
                .flat_map(|(t, ps)| ps.iter().map(|p| (*t, *p)))
                .collect()
        });
        self.reconcile(&id, reported.as_ref(), now, out);
        self.answer(&id)
    }

    /// A classic member heartbeating by this API is kept alive and changes nothing else (recorded).
    fn classic_beat(
        &mut self,
        id: &MemberId,
        req: &Heartbeat,
        now: Millis,
        out: &mut Vec<Effect>,
    ) -> Option<HeartbeatAnswer> {
        let m = self.members.get(id)?;
        m.classic.as_ref()?;
        if req.epoch != m.epoch {
            return Some(HeartbeatAnswer::refused(
                E::FencedMemberEpoch,
                stale_epoch(req.epoch, m.epoch),
            ));
        }
        let epoch = m.epoch;
        self.reset_session(id, now, out);
        Some(Self::done(id, epoch))
    }

    /// A known member at an epoch it may have: the one it is at, or the one before if the answer that advanced it was
    /// lost (then it is told again).
    fn poll_check(&mut self, id: &MemberId, req: &Heartbeat) -> Result<(), HeartbeatAnswer> {
        let group = &req.group;
        let Some(m) = self.members.get_mut(id).filter(|m| !m.left) else {
            return Err(HeartbeatAnswer::refused(
                E::UnknownMemberId,
                not_a_member(id.as_str(), group),
            ));
        };
        if req.epoch == m.epoch {
            // A request carrying all that a join does is a client that did not hear: it is told its assignment again.
            if req.rebalance_ms >= 0 && req.subscribed.is_some() && req.owned.is_some() {
                m.sent = None;
            }
            Ok(())
        } else if req.epoch == m.prev_epoch && req.epoch < m.epoch {
            m.sent = None;
            Ok(())
        } else {
            Err(HeartbeatAnswer::refused(
                E::FencedMemberEpoch,
                stale_epoch(req.epoch, m.epoch),
            ))
        }
    }

    /// The answer to a member that has been heard: its epoch, and its assignment if that is not what it was last told.
    fn answer(&mut self, id: &MemberId) -> HeartbeatAnswer {
        let Some(m) = self.members.get_mut(id) else {
            return HeartbeatAnswer::refused(E::UnknownMemberId, not_a_member(id.as_str(), ""));
        };
        let mut answer = Self::done(id, m.epoch);
        if m.sent.as_ref() != Some(&m.assigned) {
            m.sent = Some(m.assigned.clone());
            answer.assignment = Some(m.assigned.clone());
        }
        answer
    }
}
