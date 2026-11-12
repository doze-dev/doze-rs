//! The Lab's forced steps for a consumer group of the new protocol: a member's session runs out now, or the group moves to a new epoch
//! now. Each is the step the group takes for the same cause by itself (`timers`' session expiry, a member's own change).

use super::{ConsumerGroup, Topics};
use crate::groups::machine::{Answer, Effect, Force, Forced, Millis, Token};
use crate::groups::note::{Cause, Why};

impl ConsumerGroup {
    pub(super) fn force(
        &mut self,
        token: Token,
        force: &Force,
        topics: &Topics,
        now: Millis,
        out: &mut Vec<Effect>,
    ) {
        let done = match force {
            Force::Expire(id) if self.members.contains_key(id) => {
                let id = id.clone();
                self.drop_silent(&id, Cause::SessionExpired, now, out);
                Forced::Done
            }
            Force::Expire(_) => Forced::NoSuchMember,
            Force::Rebalance if self.members.is_empty() => Forced::Empty,
            Force::Rebalance => {
                let (before, was) = (self.epoch, self.members.len());
                self.bump_and_reassign(topics, now);
                self.bumped(before, Why::new(Cause::Forced, None, was), now, out);
                Forced::Done
            }
        };
        out.push(Effect::Reply(token, Answer::Forced(done)));
    }
}
