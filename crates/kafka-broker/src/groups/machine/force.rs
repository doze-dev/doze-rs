//! The Lab's forced steps for a classic group: a member's session runs out now, or a rebalance begins now. Each is the step the
//! group takes for that cause by itself (the session timer's, a membership change's), reached by the same functions.

use kafka_wire::ids::MemberId;

use super::{Answer, Effect, Force, Forced, GroupMachine, Millis, State, Token};
use crate::groups::note::{Cause, Why};

impl GroupMachine {
    pub(super) fn force(
        &mut self,
        token: Token,
        force: &Force,
        now: Millis,
        out: &mut Vec<Effect>,
    ) {
        let done = match force {
            Force::Expire(id) => self.force_expire(id, now, out),
            Force::Rebalance => self.force_rebalance(now, out),
        };
        out.push(Effect::Reply(token, Answer::Forced(done)));
    }

    /// The member's session timer runs out, whether or not a request of its is waiting on the group.
    fn force_expire(&mut self, id: &MemberId, now: Millis, out: &mut Vec<Effect>) -> Forced {
        if !self.members.contains_key(id) {
            return Forced::NoSuchMember;
        }
        self.session_expired(id, now, out);
        Forced::Done
    }

    /// The members are made to join again, as they are when one of them joins or leaves.
    fn force_rebalance(&mut self, now: Millis, out: &mut Vec<Effect>) -> Forced {
        match self.state {
            State::Empty => Forced::Empty,
            State::PreparingRebalance => Forced::AlreadyRebalancing,
            State::CompletingRebalance | State::Stable => {
                let why = Why::new(Cause::Forced, None, self.members.len());

                self.rebalance_after_change(now, why, out);
                Forced::Done
            }
        }
    }
}
