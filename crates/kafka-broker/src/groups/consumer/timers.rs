//! A member's session and its time to give up what it was told to.

use super::ConsumerGroup;
use crate::groups::machine::{Effect, Millis, TimerKey, TimerKind};
use crate::groups::note::{Cause, Why};

impl ConsumerGroup {
    /// A timer ran out. One that was re-armed since carries an old generation and does nothing.
    pub(crate) fn timer(&mut self, key: &TimerKey, now: Millis, out: &mut Vec<Effect>) {
        match &key.kind {
            TimerKind::Session(id) => {
                if self
                    .members
                    .get(id)
                    .is_some_and(|m| m.session_gen == key.generation)
                {
                    let id = id.clone();
                    self.drop_silent(&id, Cause::SessionExpired, now, out);
                }
            }
            TimerKind::Revoke(id) => {
                let overdue = self.members.get(id).is_some_and(|m| {
                    // A consumer-protocol member that has not said it gave up what it was told to; or a classic member
                    // that still holds what it is no longer to have (it gives it up only by joining again).
                    m.revoke_gen == key.generation
                        && (!m.revoking.is_empty()
                            || (m.classic.is_some()
                                && m.assigned.iter().any(|p| !m.target.contains(p))))
                });
                if overdue {
                    let id = id.clone();
                    self.drop_silent(&id, Cause::RebalanceTimeout, now, out);
                } else if let Some(m) = self.members.get_mut(id)
                    && m.revoke_gen == key.generation
                {
                    m.revoke_gen = 0;
                }
            }
            TimerKind::Rebalance | TimerKind::Hold => {}
        }
    }

    /// Removes a member that went silent or did not give up in time. If it was the last of the consumer protocol the
    /// group may now be classic again.
    pub(super) fn drop_silent(
        &mut self,
        id: &kafka_wire::ids::MemberId,
        cause: Cause,
        now: Millis,
        out: &mut Vec<Effect>,
    ) {
        let (before, was, who) = (self.epoch, self.members.len(), self.who_is(id, None));
        self.remove_member(id, out);
        self.bump_for_removal();
        self.bumped(before, Why::new(cause, who, was), now, out);
        self.check_downgrade = true;
    }
}
