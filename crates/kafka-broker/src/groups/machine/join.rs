//! JoinGroup: who is asking, whether the group can take them, and what their arrival sets going.

use std::collections::BTreeSet;

use bytes::Bytes;
use kafka_wire::ResponseError;
use kafka_wire::ids::MemberId;

use super::{
    Answer, Effect, GroupMachine, JoinAnswer, JoinRequest, Member, Millis, SESSION_MS, State, Token,
};

/// Who a JoinGroup comes from, once the id and instance it carries are understood.
enum Who {
    /// A member the group has.
    Known(MemberId),
    /// A client with an id the broker issued it earlier and the group has not yet seated.
    Issued(MemberId),
    /// Someone new, to be given an id.
    Fresh,
    /// A static member that restarted: the new id takes the place of this old one.
    Replace(MemberId),
}

type Refused = (ResponseError, Option<MemberId>);

impl Who {
    /// The member to leave out when counting the others in the group.
    fn except(&self) -> Option<&MemberId> {
        match self {
            Who::Known(id) | Who::Issued(id) | Who::Replace(id) => Some(id),
            Who::Fresh => None,
        }
    }

    /// The id to tell a client it is refused under, if it has one.
    fn known_id(&self) -> Option<MemberId> {
        self.except().cloned()
    }
}

/// Why a JoinGroup is refused before anything is asked of the group. The order is Kafka's, as recorded: the session
/// bounds come before an id is handed out.
pub(crate) fn refusal(req: &JoinRequest) -> Option<ResponseError> {
    if !SESSION_MS.contains(&req.session_ms) {
        Some(ResponseError::InvalidSessionTimeout)
    } else if req.protocols.is_empty() || req.protocol_type.is_empty() {
        Some(ResponseError::InconsistentGroupProtocol)
    } else {
        None
    }
}

impl GroupMachine {
    pub(super) fn join(
        &mut self,
        token: Token,
        req: &JoinRequest,
        now: Millis,
        out: &mut Vec<Effect>,
    ) {
        let refuse = |out: &mut Vec<Effect>, (error, id): Refused| {
            out.push(Effect::Reply(
                token,
                Answer::Join(JoinAnswer::refused(error, id)),
            ));
        };
        if let Some(error) = refusal(req) {
            return refuse(out, (error, None));
        }
        let who = match self.classify(req) {
            Ok(who) => who,
            Err(refused) => return refuse(out, refused),
        };
        if !self.compatible(req, who.except()) {
            return refuse(
                out,
                (ResponseError::InconsistentGroupProtocol, who.known_id()),
            );
        }
        match who {
            Who::Known(id) => self.rejoin(token, &id, req, now, out),
            Who::Issued(id) => self.seat(token, id, req, now, out),
            Who::Fresh => {
                let id = (self.new_id)(req.instance.as_ref());
                self.seat(token, id, req, now, out);
            }
            Who::Replace(old) => {
                let new = (self.new_id)(req.instance.as_ref());
                self.replace_static(&old, &new, out);
                self.rejoin(token, &new, req, now, out);
            }
        }
    }

    /// Whether this join would put a member in the group: it is not refused for any reason. Changes nothing.
    #[must_use]
    pub(crate) fn would_seat(&self, req: &JoinRequest) -> bool {
        refusal(req).is_none()
            && self
                .classify(req)
                .is_ok_and(|who| self.compatible(req, who.except()))
    }

    /// Reads the member id and instance id of a request.
    fn classify(&self, req: &JoinRequest) -> Result<Who, Refused> {
        use ResponseError as E;
        match (&req.instance, &req.member_id) {
            (Some(inst), given) => match (self.statics.get(inst), given) {
                (Some(held), Some(id)) if id != held => {
                    Err((E::FencedInstanceId, Some(id.clone())))
                }
                (Some(held), Some(_)) => Ok(Who::Known(held.clone())),
                (Some(old), None) => Ok(Who::Replace(old.clone())),
                (None, Some(id)) => Err((E::UnknownMemberId, Some(id.clone()))),
                (None, None) => Ok(Who::Fresh),
            },
            (None, Some(id)) if self.members.contains_key(id) => Ok(Who::Known(id.clone())),
            (None, Some(id)) if req.issued => Ok(Who::Issued(id.clone())),
            (None, Some(id)) => Err((E::UnknownMemberId, Some(id.clone()))),
            (None, None) => Ok(Who::Fresh),
        }
    }

    /// Whether the group can take a member offering these protocols: an empty group takes any; otherwise the type is
    /// the group's, and (if there are other members) some protocol is one all of them offer. The member rejoining is
    /// not counted among the others, but the type is held to even when it is the only member.
    fn compatible(&self, req: &JoinRequest, except: Option<&MemberId>) -> bool {
        if self.members.is_empty() {
            return true;
        }
        if self.protocol_type.as_deref() != Some(req.protocol_type.as_str()) {
            return false;
        }
        let mut others = self
            .members
            .values()
            .filter(|m| Some(&m.id) != except)
            .map(|m| {
                m.protocols
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect::<BTreeSet<_>>()
            });
        let Some(first) = others.next() else {
            return true;
        };
        let common = others.fold(first, |acc, s| acc.intersection(&s).copied().collect());
        req.protocols
            .iter()
            .any(|p| common.contains(p.name.as_str()))
    }

    /// A member the group did not have is added, and its arrival starts or joins a rebalance.
    fn seat(
        &mut self,
        token: Token,
        id: MemberId,
        req: &JoinRequest,
        now: Millis,
        out: &mut Vec<Effect>,
    ) {
        if self.members.is_empty() {
            self.protocol_type = Some(req.protocol_type.clone());
            // The first member to join an empty group leads it (as in Kafka), whatever its id sorts as.
            self.leader = Some(id.clone());
        }
        if let Some(inst) = &req.instance {
            self.statics.insert(inst.clone(), id.clone());
        }
        let mut member = new_member(&id, req);
        member.joined = Some(token);
        self.members.insert(id.clone(), member);
        self.arm_session(&id, now, out);
        match self.state {
            State::Empty => self.begin_rebalance(now, true, out),
            State::CompletingRebalance | State::Stable => self.begin_rebalance(now, false, out),
            State::PreparingRebalance => {
                // A member that was not in the group arrived while it was holding for latecomers.
                if let Some(hold) = self.rebalance.as_mut().and_then(|r| r.hold.as_mut()) {
                    hold.new_member = true;
                }
            }
        }
        self.maybe_complete(now, out);
    }

    /// A member the group has asks to join again: nothing changes if it says the same as before and the group is
    /// running; otherwise a rebalance begins.
    fn rejoin(
        &mut self,
        token: Token,
        id: &MemberId,
        req: &JoinRequest,
        now: Millis,
        out: &mut Vec<Effect>,
    ) {
        let leads = self.leader.as_ref() == Some(id);
        let same = {
            let Some(member) = self.members.get_mut(id) else {
                return;
            };
            member.session_ms = to_ms(req.session_ms);
            member.rebalance_ms = to_ms(req.rebalance_ms);
            member.rights.clone_from(&req.rights);
            member.protocols == req.protocols
        };
        let answer_now = match self.state {
            State::CompletingRebalance => same,
            State::Stable => same && !leads,
            State::Empty | State::PreparingRebalance => false,
        };
        if answer_now {
            let answer = self.join_answer(id);
            out.push(Effect::Reply(token, Answer::Join(answer)));
        } else {
            if let Some(member) = self.members.get_mut(id) {
                member.protocols.clone_from(&req.protocols);
                // A join sent again replaces the one waiting; the earlier is never answered.
                if let Some(old) = member.joined.replace(token) {
                    out.push(Effect::Forget(old));
                }
            }
            self.begin_rebalance(now, false, out);
        }
        self.arm_session(id, now, out);
        self.maybe_complete(now, out);
    }

    /// The restarted static member `new` takes the place of `old`: its assignment and its leadership. Whatever of
    /// the old one was waiting is told it was fenced.
    fn replace_static(&mut self, old: &MemberId, new: &MemberId, out: &mut Vec<Effect>) {
        let Some(mut member) = self.members.remove(old) else {
            return;
        };
        out.push(Effect::Disarm(super::TimerKind::Session(old.clone())));
        if let Some(token) = member.joined.take() {
            out.push(Effect::Reply(
                token,
                Answer::Join(JoinAnswer::refused(ResponseError::FencedInstanceId, None)),
            ));
        }
        if let Some(token) = member.syncing.take() {
            out.push(Effect::Reply(
                token,
                Answer::Sync(super::SyncAnswer::refused(ResponseError::FencedInstanceId)),
            ));
        }
        if let Some(inst) = &member.instance {
            self.statics.insert(inst.clone(), new.clone());
        }
        if self.leader.as_ref() == Some(old) {
            self.leader = Some(new.clone());
        }
        member.id = new.clone();
        self.members.insert(new.clone(), member);
    }
}

fn to_ms(ms: i32) -> u64 {
    u64::try_from(ms).unwrap_or(0)
}

fn new_member(id: &MemberId, req: &JoinRequest) -> Member {
    Member {
        id: id.clone(),
        instance: req.instance.clone(),
        client_id: req.client_id.clone(),
        client_host: req.client_host.clone(),
        session_ms: to_ms(req.session_ms),
        rebalance_ms: to_ms(req.rebalance_ms),
        protocols: req.protocols.clone(),
        assignment: Bytes::new(),
        previous: Bytes::new(),
        joined: None,
        syncing: None,
        session_gen: 0,
        heard: Millis(0),
        rights: req.rights.clone(),
    }
}
