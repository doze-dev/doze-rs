//! What a streams group does besides a heartbeat: computing the targets, its timers running out (a session, a rebalance
//! timeout, the initial rebalance delay), the topics changing, and the Lab's forced steps.

use std::collections::BTreeMap;

use kafka_wire::ids::MemberId;

use super::StreamsGroup;
use super::assign::{Seat, SubTasks, assign};
use crate::groups::machine::{Effect, Millis, TimerKey, TimerKind};
use crate::groups::note::{CAPTURE, Cause, Named, What, Why, count, say};

impl StreamsGroup {
    /// Computes every member's target for the group epoch: by the sticky assignor if the topology is ready, else nothing
    /// for anybody (Kafka's `TargetAssignmentBuilder`).
    pub(super) fn compute_targets(&mut self, now: Millis) {
        let standbys = self.assignment_configs.as_ref().map_or(0, |c| c.0);
        self.targets = match self.configured.as_ref().filter(|c| c.ready()) {
            Some(c) => {
                let subs: BTreeMap<String, SubTasks> = c
                    .subtopologies
                    .iter()
                    .map(|(id, s)| {
                        let sub = SubTasks {
                            tasks: s.tasks,
                            stateful: s.stateful,
                        };
                        (id.clone(), sub)
                    })
                    .collect();
                let seats: Vec<Seat<'_>> = self
                    .members
                    .values()
                    .map(|m| Seat {
                        id: &m.id,
                        process: &m.process,
                        current: &m.held.tasks,
                        offsets: &m.offsets,
                    })
                    .collect();
                assign(&subs, &seats, standbys)
            }
            None => self
                .members
                .keys()
                .map(|id| (id.clone(), super::Tasks::default()))
                .collect(),
        };
        self.assignment_epoch = self.epoch;
        self.assigned_at = Some(now);
    }

    /// A timer ran out: a member's session, its time to revoke (if it is still at the epoch it was armed at), or the initial
    /// rebalance delay, after which the targets are computed if they may be.
    pub(crate) fn timer(&mut self, key: &TimerKey, now: Millis, out: &mut Vec<Effect>) {
        match &key.kind {
            TimerKind::Session(id) if self.members.get(id).is_some_and(|m| m.session_gen == key.generation) => {
                let id = id.clone();
                self.remove(&id, Cause::SessionExpired, now, out);
            }
            TimerKind::Revoke(id)
                if self
                    .members
                    .get(id)
                    .is_some_and(|m| m.revoke_gen == key.generation && m.revoke_epoch == m.held.epoch) =>
            {
                let id = id.clone();
                self.remove(&id, Cause::RebalanceTimeout, now, out);
            }
            TimerKind::Hold if self.delaying => {
                self.delaying = false;
                let configured = self.configured.is_some();
                if !self.members.is_empty() && configured && self.epoch > self.assignment_epoch && self.may_assign(now) {
                    self.compute_targets(now);
                }
            }
            _ => {}
        }
    }

    /// A topic was made, grown or deleted: the next heartbeat looks again at the topics the topology names.
    pub(crate) fn topics_changed(&mut self) {}

    /// The Lab's steps: a member's session runs out, or the epoch moves (the next heartbeat computes new targets).
    pub(crate) fn force_expire(&mut self, id: &MemberId, now: Millis, out: &mut Vec<Effect>) -> bool {
        if !self.members.contains_key(id) {
            return false;
        }
        self.remove(id, Cause::SessionExpired, now, out);
        true
    }

    pub(crate) fn force_bump(&mut self, now: Millis, out: &mut Vec<Effect>) -> bool {
        if self.members.is_empty() {
            return false;
        }
        self.epoch += 1;
        self.note_epoch(Cause::Forced, None, self.members.len(), now, out);
        true
    }

    pub(super) fn who_is(&self, id: &MemberId) -> Option<Named> {
        if !CAPTURE {
            return None;
        }
        self.members
            .get(id)
            .map(|m| Named::new(id, &m.client_id, None))
    }

    pub(super) fn note_epoch(&self, cause: Cause, who: Option<Named>, was: usize, now: Millis, out: &mut Vec<Effect>) {
        let why = Why::new(cause, who, was);
        say(out, now, || What::Epoch {
            why,
            epoch: self.epoch,
            after: count(self.members.len()),
        });
    }
}
