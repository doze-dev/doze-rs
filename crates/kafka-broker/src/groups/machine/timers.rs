//! What happens when a timer runs out.

use super::{Effect, GroupMachine, Millis, TimerKey, TimerKind};

impl GroupMachine {
    pub(super) fn timer(&mut self, key: &TimerKey, now: Millis, out: &mut Vec<Effect>) {
        match &key.kind {
            TimerKind::Session(id) => {
                let Some(member) = self.members.get(id) else {
                    return;
                };
                if member.session_gen != key.generation {
                    return;
                }
                if member.joined.is_some() || member.syncing.is_some() {
                    // A member whose request is waiting on the group is not silent: the group is.
                    self.arm_session(id, now, out);
                    return;
                }
                let id = id.clone();
                self.drop_member(&id, out);
                self.rebalance_after_change(now, out);
            }
            TimerKind::Rebalance => {
                if self
                    .rebalance
                    .as_ref()
                    .is_some_and(|r| r.generation == key.generation)
                {
                    self.complete_join(now, out);
                }
            }
            TimerKind::Hold => self.window_ended(key.generation, now, out),
        }
    }

    /// The initial delay's window is over: if a member arrived during it, wait again; if not, go on.
    fn window_ended(&mut self, generation: u64, now: Millis, out: &mut Vec<Effect>) {
        let Some(rebalance) = self.rebalance.as_mut() else {
            return;
        };
        let deadline = rebalance.deadline;
        let ended = match rebalance.hold.take() {
            Some(hold) if hold.generation == generation => hold,
            other => {
                rebalance.hold = other;
                return;
            }
        };
        if ended.new_member {
            let next = self.open_window(now, deadline, false, out);
            if let Some(r) = self.rebalance.as_mut() {
                r.hold = Some(next);
            }
        } else {
            self.maybe_complete(now, out);
        }
    }
}
