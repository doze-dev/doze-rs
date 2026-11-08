//! JoinGroup: who is asking, whether the group can take them, and what their arrival sets going.

use std::collections::BTreeSet;

use bytes::Bytes;
use kafka_wire::ResponseError;
use kafka_wire::ids::MemberId;

use super::{
    Answer, Effect, GroupMachine, JoinAnswer, JoinRequest, Member, Millis, PENDING_TTL_MS,
    SESSION_MS, State, Token,
};

/// Who a JoinGroup comes from, once the id and instance it carries are understood.
enum Who {
    /// A member the group has.
    Known(MemberId),
    /// Someone the group does not have yet, under this id.
    New(MemberId),
    /// A static member that restarted: the new id takes the place of the old.
    Replace { old: MemberId, new: MemberId },
}

type Refused = (ResponseError, Option<MemberId>);

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
        // The order is Kafka's, as recorded: the session bounds come before an id is handed out.
        if !SESSION_MS.contains(&req.session_ms) {
            return refuse(out, (ResponseError::InvalidSessionTimeout, None));
        }
        if req.protocols.is_empty() || req.protocol_type.is_empty() {
            return refuse(out, (ResponseError::InconsistentGroupProtocol, None));
        }
        let who = match self.identify(req, now) {
            Ok(who) => who,
            Err(refused) => return refuse(out, refused),
        };
        let except = match &who {
            Who::Known(id) | Who::New(id) => Some(id),
            Who::Replace { old, .. } => Some(old),
        };
        if !self.compatible(req, except) {
            let id = match &who {
                Who::Known(id) | Who::New(id) | Who::Replace { new: id, .. } => id.clone(),
            };
            return refuse(out, (ResponseError::InconsistentGroupProtocol, Some(id)));
        }
        match who {
            Who::New(id) => self.seat(token, id, req, now, out),
            Who::Known(id) => self.rejoin(token, &id, req, now, out),
            Who::Replace { old, new } => {
                self.replace_static(&old, &new, out);
                self.rejoin(token, &new, req, now, out);
            }
        }
    }

    /// Reads the member id and instance id of a request.
    fn identify(&mut self, req: &JoinRequest, now: Millis) -> Result<Who, Refused> {
        use ResponseError as E;
        match (&req.instance, &req.member_id) {
            (Some(inst), given) => match (self.statics.get(inst).cloned(), given) {
                (Some(held), Some(id)) if *id != held => {
                    Err((E::FencedInstanceId, Some(id.clone())))
                }
                (Some(held), Some(_)) => Ok(Who::Known(held)),
                (Some(old), None) => Ok(Who::Replace {
                    old,
                    new: (self.new_id)(Some(inst)),
                }),
                (None, Some(id)) => Err((E::UnknownMemberId, Some(id.clone()))),
                (None, None) => Ok(Who::New((self.new_id)(Some(inst)))),
            },
            (None, Some(id)) => {
                self.forget_stale_pending(now);
                if self.members.contains_key(id) {
                    Ok(Who::Known(id.clone()))
                } else if self.pending.contains_key(id) {
                    Ok(Who::New(id.clone()))
                } else {
                    Err((E::UnknownMemberId, Some(id.clone())))
                }
            }
            (None, None) if req.id_first => {
                self.forget_stale_pending(now);
                let id = (self.new_id)(None);
                self.pending.insert(id.clone(), now);
                Err((E::MemberIdRequired, Some(id)))
            }
            (None, None) => Ok(Who::New((self.new_id)(None))),
        }
    }

    fn forget_stale_pending(&mut self, now: Millis) {
        self.pending
            .retain(|_, issued| now.since(*issued) < PENDING_TTL_MS);
    }

    /// Whether the group can take a member offering these protocols: the type is the group's and some protocol is
    /// one every member already in the group offers. (A rejoining member is not counted among the others.)
    fn compatible(&self, req: &JoinRequest, except: Option<&MemberId>) -> bool {
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
        self.protocol_type.as_deref() == Some(req.protocol_type.as_str())
            && req
                .protocols
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
        self.pending.remove(&id);
        if self.members.is_empty() {
            self.protocol_type = Some(req.protocol_type.clone());
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
                member.joined = Some(token);
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
        session_ms: to_ms(req.session_ms),
        rebalance_ms: to_ms(req.rebalance_ms),
        protocols: req.protocols.clone(),
        assignment: Bytes::new(),
        joined: None,
        syncing: None,
        session_gen: 0,
    }
}
