//! A member's session and its time to give up what it was told to.

use super::ConsumerGroup;
use crate::groups::machine::{Effect, Millis, TimerKey, TimerKind};

impl ConsumerGroup {
    /// A timer ran out. One that was re-armed since carries an old generation and does nothing.
    pub(crate) fn timer(&mut self, key: &TimerKey, _now: Millis, out: &mut Vec<Effect>) {
        match &key.kind {
            TimerKind::Session(id) => {
                if self
                    .members
                    .get(id)
                    .is_some_and(|m| m.session_gen == key.generation)
                {
                    let id = id.clone();
                    self.remove_member(&id, out);
                    self.bump_for_removal();
                }
            }
            TimerKind::Revoke(id) => {
                if self
                    .members
                    .get(id)
                    .is_some_and(|m| m.revoke_gen == key.generation && !m.revoking.is_empty())
                {
                    let id = id.clone();
                    self.remove_member(&id, out);
                    self.bump_for_removal();
                }
            }
            TimerKind::Rebalance | TimerKind::Hold => {}
        }
    }
}
