//! A classic member's JoinGroup to a consumer group.

use kafka_wire::ResponseError as E;
use kafka_wire::ids::MemberId;

use super::super::member::{ClassicSeat, Member};
use super::super::{ConsumerGroup, Topics};
use super::{partitions_of, subscription};
use crate::groups::machine::{Answer, Effect, JoinAnswer, JoinRequest, Millis, Token};

impl ConsumerGroup {
    /// A classic JoinGroup to a consumer group: its protocol type must be `consumer`, it must offer some protocol, and
    /// share one with the other classic members.
    pub(crate) fn protocol_refusal(&self, req: &JoinRequest) -> Option<E> {
        (req.protocol_type != "consumer"
            || req.protocols.is_empty()
            || !self.classic_members_share(req))
        .then_some(E::InconsistentGroupProtocol)
    }

    fn classic_members_share(&self, req: &JoinRequest) -> bool {
        req.protocols.iter().any(|offered| {
            self.members
                .values()
                .filter(|o| Some(&o.id) != req.member_id.as_ref())
                .filter_map(|o| o.classic.as_ref())
                .all(|seat| seat.protocols.iter().any(|p| p.name == offered.name))
        })
    }

    pub(in crate::groups::consumer) fn classic_join(
        &mut self,
        token: Token,
        req: &JoinRequest,
        topics: &Topics,
        now: Millis,
        out: &mut Vec<Effect>,
    ) {
        let answer = self.join_classic(req, topics, now, out);
        out.push(Effect::Reply(token, Answer::Join(answer)));
    }

    fn join_classic(
        &mut self,
        req: &JoinRequest,
        topics: &Topics,
        now: Millis,
        out: &mut Vec<Effect>,
    ) -> JoinAnswer {
        let refuse = |e: E| JoinAnswer::refused(e, req.member_id.clone());
        // A group of consumers has one protocol type, and its classic members share a protocol.
        if let Some(e) = self.protocol_refusal(req) {
            return refuse(e);
        }
        let mut predecessor: Option<MemberId> = None;
        let id = match &req.member_id {
            Some(id) => id.clone(),
            None => {
                // A static member joins in one step, in the place of the holder of its instance; a version before 4
                // is joined in one step too (the later ones were handed an id first).
                if let Some(instance) = &req.instance {
                    predecessor = self.holder_of(instance).cloned();
                }
                (self.new_id)(req.instance.as_ref())
            }
        };
        // Read once the member has an id. One that cannot be read seats nobody: Kafka fails to read it, with an
        // exception it does not catch, and answers an error with no member id.
        let Some((sub, protocol)) = subscription(&req.protocols) else {
            return JoinAnswer::refused(E::UnknownServerError, None);
        };
        if let Some(refused) = self.id_refusal(req, &id) {
            return refused;
        }
        let (mut member, mut changed, replaced_consumer) =
            self.classic_member(&id, predecessor, out);
        // A member asking in holds what its JoinGroup says it still owns and has given up the rest.
        let owned = partitions_of(&sub.owned, topics);
        member.assigned.retain(|p| owned.contains(p));
        member.instance.clone_from(&req.instance);
        member.client_id.clone_from(&req.client_id);
        if member.subscribed != sub.topics {
            member.subscribed.clone_from(&sub.topics);
            changed = true;
        }
        member.classic = Some(ClassicSeat {
            protocols: req.protocols.clone(),
            protocol: protocol.clone(),
            version: sub.version,
            session_ms: u64::try_from(req.session_ms).unwrap_or(0),
        });
        member.rebalance_ms = if req.rebalance_ms > 0 {
            req.rebalance_ms
        } else {
            req.session_ms
        };
        member.left = false;
        self.members.insert(id.clone(), member);
        self.reset_session(&id, now, out);
        if changed {
            self.bump_and_reassign(topics, now);
        } else {
            self.assign_if_due(topics, now);
        }
        self.reconcile(&id, Some(&owned), now, out);
        self.end_revocation_if_given_up(&id, out);
        self.arm_classic_revocations(now, out);
        // The member it replaced may have been the last of the consumer protocol.
        self.check_downgrade |= replaced_consumer;
        let generation = self.members.get(&id).map_or(-1, |m| m.epoch);
        JoinAnswer {
            error: None,
            generation,
            protocol_type: Some("consumer".to_owned()),
            protocol: Some(protocol),
            leader: None,
            member_id: Some(id),
            members: Vec::new(),
        }
    }

    /// Having joined again, a classic member has given up what it was to: the deadline it had for that is over.
    fn end_revocation_if_given_up(&mut self, id: &MemberId, out: &mut Vec<Effect>) {
        if let Some(m) = self.members.get_mut(id)
            && m.revoke_gen != 0
            && m.revoking.is_empty()
            && m.assigned.iter().all(|p| m.target.contains(p))
        {
            m.revoke_gen = 0;
            out.push(Effect::Disarm(crate::groups::machine::TimerKind::Revoke(
                id.clone(),
            )));
        }
    }

    /// A classic JoinGroup naming an id that is not for it: a consumer-protocol member's, or one a restarted instance has
    /// replaced (answered with no member id).
    fn id_refusal(&self, req: &JoinRequest, id: &MemberId) -> Option<JoinAnswer> {
        match self.members.get(id) {
            Some(m) if m.classic.is_none() => Some(JoinAnswer::refused(
                E::UnknownMemberId,
                req.member_id.clone(),
            )),
            None if req.member_id.is_some()
                && self.static_code(id, req.instance.as_ref()) == Some(E::FencedInstanceId) =>
            {
                Some(JoinAnswer::refused(E::FencedInstanceId, None))
            }
            _ => None,
        }
    }

    /// The member a classic JoinGroup is about: the restarted static instance's predecessor's place taken (it has what the
    /// old one had), the one that is already here, or a new one. Whether the group changes by it, and whether the member
    /// replaced was of the consumer protocol.
    fn classic_member(
        &mut self,
        id: &MemberId,
        predecessor: Option<MemberId>,
        out: &mut Vec<Effect>,
    ) -> (Member, bool, bool) {
        if let Some(old) = predecessor.and_then(|p| self.members.remove(&p)) {
            Self::disarm(&old.id, out);
            let mut m = Member::new(id.clone());
            m.target = old.target;
            m.assigned = old.assigned;
            m.revoking = old.revoking;
            m.epoch = old.epoch;
            m.prev_epoch = old.prev_epoch;
            m.subscribed = old.subscribed;
            (m, old.pattern.is_some(), old.classic.is_none())
        } else if let Some(known) = self.members.remove(id) {
            (known, false, false)
        } else {
            (Member::new(id.clone()), true, false)
        }
    }
}
