//! Moving members to their targets, one safe step per heartbeat, and the group's epochs.

use std::collections::{BTreeMap, HashSet};

use kafka_wire::ids::MemberId;

use super::assign::{Assignor, Partition, Partitions, Subscribed, assign};
use super::member::Member;
use super::{ConsumerGroup, Topics};
use crate::groups::machine::{Effect, Millis, TimerKind};

impl ConsumerGroup {
    /// Moves one member a step towards its target, never handing it a partition another member still holds.
    /// `reported` is what the member says it owns; `None` if it did not say.
    pub(super) fn reconcile(
        &mut self,
        id: &MemberId,
        reported: Option<&Partitions>,
        now: Millis,
        out: &mut Vec<Effect>,
    ) {
        let target_epoch = self.target_epoch;
        let Some(m) = self.members.get_mut(id) else {
            return;
        };
        // 1. Waiting for it to give partitions up: only its own report that it holds none of them counts.
        if !m.revoking.is_empty() {
            match reported {
                Some(r) if r.is_disjoint(&m.revoking) => {
                    m.revoking.clear();
                    m.revoke_gen = 0;
                    out.push(Effect::Disarm(TimerKind::Revoke(id.clone())));
                }
                _ => return,
            }
        }
        if m.epoch == target_epoch && m.assigned == m.target {
            return;
        }
        // 2. Anything it holds that is no longer its target is revoked first, and its epoch does not move until it says
        // it has given them up.
        let (keep, revoke): (Partitions, Partitions) =
            m.assigned.iter().partition(|p| m.target.contains(*p));
        if !revoke.is_empty() {
            m.assigned = keep;
            m.revoking = revoke;
            self.arm_revoke(id, now, out);
            return;
        }
        // 3. Nothing to give up: take every target partition nobody else holds, and move to the group's epoch.
        // What is still held elsewhere arrives on a later heartbeat, once its holder has let go.
        let wanted: Vec<Partition> = m
            .target
            .iter()
            .filter(|p| !keep.contains(*p))
            .copied()
            .collect();
        let free: Vec<Partition> = wanted.into_iter().filter(|p| self.free(id, p)).collect();
        let Some(m) = self.members.get_mut(id) else {
            return;
        };
        let mut assigned = keep;
        assigned.extend(free);
        m.assigned = assigned;
        if m.epoch != target_epoch {
            m.prev_epoch = m.epoch;
            m.epoch = target_epoch;
        }
    }

    /// No member but `me` holds `partition`, as assigned or as a revocation not yet acknowledged.
    pub(super) fn free(&self, me: &MemberId, partition: &Partition) -> bool {
        self.members.values().all(|o| {
            o.id == *me || !(o.assigned.contains(partition) || o.revoking.contains(partition))
        })
    }

    /// Starts the member's rebalance timeout: how long it has to say it has given up what it was told to, before it is
    /// removed and its partitions freed.
    pub(super) fn arm_revoke(&mut self, id: &MemberId, now: Millis, out: &mut Vec<Effect>) {
        let Some(m) = self.members.get(id) else {
            return;
        };
        let ms = u64::try_from(m.rebalance_ms)
            .ok()
            .filter(|ms| *ms > 0)
            .unwrap_or(self.cfg.session_ms);
        let generation = self.arm(TimerKind::Revoke(id.clone()), now.plus(ms), out);
        if let Some(m) = self.members.get_mut(id) {
            m.revoke_gen = generation;
        }
    }

    /// Starts (or restarts) the member's session timer.
    pub(super) fn reset_session(&mut self, id: &MemberId, now: Millis, out: &mut Vec<Effect>) {
        let session = self.members.get(id).map_or(self.cfg.session_ms, |m| {
            m.classic
                .as_ref()
                .map_or(self.cfg.session_ms, |c| c.session_ms)
        });
        let generation = self.arm(TimerKind::Session(id.clone()), now.plus(session), out);
        if let Some(m) = self.members.get_mut(id) {
            m.session_gen = generation;
        }
    }

    pub(super) fn disarm(id: &MemberId, out: &mut Vec<Effect>) {
        out.push(Effect::Disarm(TimerKind::Session(id.clone())));
        out.push(Effect::Disarm(TimerKind::Revoke(id.clone())));
    }

    pub(super) fn remove_member(&mut self, id: &MemberId, out: &mut Vec<Effect>) {
        if self.members.remove(id).is_some() {
            Self::disarm(id, out);
        }
        if self.members.is_empty() {
            self.epoch = self.epoch.saturating_add(1);
        }
    }

    /// The epoch moves for a member that left or was removed; the targets wait for the next heartbeat.
    pub(super) fn bump_for_removal(&mut self) {
        if !self.members.is_empty() {
            self.epoch = self.epoch.saturating_add(1);
        }
    }

    /// The epoch moves for a change a member's own heartbeat made, and the targets are computed if it is time.
    pub(super) fn bump_and_reassign(&mut self, topics: &Topics, now: Millis) {
        if self.members.is_empty() {
            return;
        }
        self.epoch = self.epoch.saturating_add(1);
        self.assign_if_due(topics, now);
    }

    /// Computes the targets for the group's epoch, if the members are behind it and the last computation is at least
    /// the assignment interval ago.
    pub(super) fn assign_if_due(&mut self, topics: &Topics, now: Millis) -> bool {
        let due = self
            .assigned_at
            .is_none_or(|at| now.0.saturating_sub(at.0) >= self.cfg.assign_interval_ms);
        if self.members.is_empty() || self.target_epoch >= self.epoch || !due {
            return false;
        }
        self.compute_targets(topics);
        self.target_epoch = self.epoch;
        self.assigned_at = Some(now);
        true
    }

    /// Runs the assignor now, for the members and topics there are. `true`: a target changed.
    fn compute_targets(&mut self, topics: &Topics) -> bool {
        let (mut uniform, mut range) = (0_usize, 0_usize);
        for m in self.members.values() {
            match m.assignor {
                Some(Assignor::Uniform) => uniform += 1,
                Some(Assignor::Range) => range += 1,
                None => {}
            }
        }
        self.assignor = if range > uniform {
            Assignor::Range
        } else {
            Assignor::Uniform
        };
        let ids: Vec<MemberId> = self.members.keys().cloned().collect();
        let previous: BTreeMap<MemberId, Partitions> = self
            .members
            .iter()
            .map(|(id, m)| (id.clone(), m.target.clone()))
            .collect();
        let targets = assign(self.assignor, &ids, &self.subscribed(topics), &previous);
        let mut changed = false;
        for (id, target) in targets {
            if let Some(m) = self.members.get_mut(&id) {
                changed |= m.target != target;
                m.target = target;
            }
        }
        changed
    }

    /// The topics that exist and some member wants, by name, each with who wants it.
    fn subscribed(&self, topics: &Topics) -> Vec<Subscribed> {
        // Each member's names as a set once, not searched as a list for every topic.
        let names: Vec<(&Member, HashSet<&str>)> = self
            .members
            .values()
            .map(|m| (m, m.subscribed.iter().map(String::as_str).collect()))
            .collect();
        topics
            .all()
            .filter(|t| t.partitions > 0)
            .filter_map(|t| {
                let subscribers: Vec<MemberId> = names
                    .iter()
                    .filter(|(m, set)| set.contains(t.name.as_str()) || m.pattern_takes(&t.name))
                    .map(|(m, _)| m.id.clone())
                    .collect();
                (!subscribers.is_empty()).then_some(Subscribed {
                    id: t.id,
                    partitions: t.partitions,
                    subscribers,
                })
            })
            .collect()
    }

    /// A topic was made, grown or deleted: members that wanted it before it existed, or want it no more, are told by a
    /// new epoch when their targets change, since the targets are otherwise computed only when membership changes.
    pub(crate) fn topics_changed(&mut self, topics: &Topics, now: Millis, out: &mut Vec<Effect>) {
        if self.members.is_empty() {
            return;
        }
        if self.compute_targets(topics) {
            self.epoch = self.epoch.saturating_add(1);
        }
        self.target_epoch = self.epoch;
        self.assigned_at = Some(now);
        self.arm_classic_revocations(now, out);
    }
}
