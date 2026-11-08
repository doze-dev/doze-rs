//! Starting a rebalance, finishing it, and what it decides: the protocol and the leader.

use std::collections::BTreeMap;

use kafka_wire::ResponseError;
use kafka_wire::ids::MemberId;

use super::{
    Answer, DEFAULT_REBALANCE_MS, Effect, GroupMachine, Hold, JoinAnswer, Millis, Rebalance, State,
    SyncAnswer, TimerKey, TimerKind,
};

impl GroupMachine {
    /// Moves to `PreparingRebalance`: the members have until the longest of their rebalance timeouts to join again.
    /// A group that was empty also waits out the initial delay, so that members starting together share one
    /// generation. Parked SyncGroups are told to join again.
    pub(super) fn begin_rebalance(&mut self, now: Millis, hold: bool, out: &mut Vec<Effect>) {
        if self.state == State::PreparingRebalance {
            return;
        }
        for m in self.members.values_mut() {
            if let Some(token) = m.syncing.take() {
                out.push(Effect::Reply(
                    token,
                    Answer::Sync(SyncAnswer::refused(ResponseError::RebalanceInProgress)),
                ));
            }
        }
        self.state = State::PreparingRebalance;
        let longest = self
            .members
            .values()
            .map(|m| m.rebalance_ms)
            .max()
            .unwrap_or(DEFAULT_REBALANCE_MS);
        let deadline = now.plus(longest);
        let generation = self.next_arming();
        out.push(Effect::Arm {
            key: TimerKey {
                kind: TimerKind::Rebalance,
                generation,
            },
            at: deadline,
        });
        let hold = (hold && self.initial_delay_ms > 0)
            .then(|| self.open_window(now, deadline, false, out));
        self.rebalance = Some(Rebalance {
            deadline,
            generation,
            hold,
        });
    }

    /// A window of the initial delay from `now`, cut short by the deadline.
    pub(super) fn open_window(
        &mut self,
        now: Millis,
        deadline: Millis,
        new_member: bool,
        out: &mut Vec<Effect>,
    ) -> Hold {
        let until = now.plus(self.initial_delay_ms).min(deadline);
        let generation = self.next_arming();
        out.push(Effect::Arm {
            key: TimerKey {
                kind: TimerKind::Hold,
                generation,
            },
            at: until,
        });
        Hold {
            new_member,
            generation,
        }
    }

    /// Completes the rebalance if every member has joined again and the group is not holding for latecomers.
    pub(super) fn maybe_complete(&mut self, now: Millis, out: &mut Vec<Effect>) {
        let holding = self.rebalance.as_ref().is_none_or(|r| r.hold.is_some());
        // With nobody left there is nobody to wait for, whatever the delay: the group is empty.
        let nobody = self.members.is_empty();
        if self.state == State::PreparingRebalance
            && (nobody || (!holding && self.members.values().all(|m| m.joined.is_some())))
        {
            self.complete_join(now, out);
        }
    }

    /// Ends the wait: members that did not join again are dropped, the generation goes up, and everyone who
    /// joined is told the protocol and the leader. With nobody left the group is empty.
    pub(super) fn complete_join(&mut self, now: Millis, out: &mut Vec<Effect>) {
        self.rebalance = None;
        out.push(Effect::Disarm(TimerKind::Rebalance));
        out.push(Effect::Disarm(TimerKind::Hold));
        let silent: Vec<MemberId> = self
            .members
            .values()
            .filter(|m| m.joined.is_none())
            .map(|m| m.id.clone())
            .collect();
        for id in &silent {
            self.drop_member(id, out);
        }
        self.generation = self.generation.wrapping_add(1).max(0);
        if self.members.is_empty() {
            self.state = State::Empty;
            self.protocol = None;
            self.leader = None;
            return;
        }
        let Some(leader) = self.elect() else {
            return;
        };
        self.protocol = Some(self.vote());
        self.leader = Some(leader);
        self.state = State::CompletingRebalance;
        let ids: Vec<MemberId> = self.members.keys().cloned().collect();
        for id in ids {
            let answer = self.join_answer(&id);
            if let Some(m) = self.members.get_mut(&id) {
                m.assignment = bytes::Bytes::new();
                if let Some(token) = m.joined.take() {
                    out.push(Effect::Reply(token, Answer::Join(answer)));
                }
            }
            self.arm_session(&id, now, out);
        }
    }

    /// The protocol every member supports that the most members put first among those. Ties go to the lower name.
    fn vote(&self) -> String {
        let mut everyone = self.members.values().map(|m| {
            m.protocols
                .iter()
                .map(|p| p.name.as_str())
                .collect::<std::collections::BTreeSet<_>>()
        });
        let common = everyone
            .next()
            .map(|first| everyone.fold(first, |acc, s| acc.intersection(&s).copied().collect()))
            .unwrap_or_default();
        let mut votes: BTreeMap<&str, usize> = BTreeMap::new();
        for m in self.members.values() {
            if let Some(p) = m
                .protocols
                .iter()
                .find(|p| common.contains(p.name.as_str()))
            {
                *votes.entry(p.name.as_str()).or_default() += 1;
            }
        }
        votes
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(a.0)))
            .map(|(name, _)| name.to_owned())
            .unwrap_or_default()
    }

    /// The leader stays if it is still a member; otherwise the lowest member id leads.
    fn elect(&self) -> Option<MemberId> {
        match &self.leader {
            Some(old) if self.members.contains_key(old) => Some(old.clone()),
            _ => self.members.keys().next().cloned(),
        }
    }

    /// Takes a member out of the group: its timer, its instance, and whatever of its requests is waiting (told it
    /// is unknown now).
    pub(super) fn drop_member(&mut self, id: &MemberId, out: &mut Vec<Effect>) {
        let Some(m) = self.members.remove(id) else {
            return;
        };
        if let Some(inst) = &m.instance {
            self.statics.remove(inst);
        }
        out.push(Effect::Disarm(TimerKind::Session(id.clone())));
        if let Some(token) = m.joined {
            out.push(Effect::Reply(
                token,
                Answer::Join(JoinAnswer::refused(ResponseError::UnknownMemberId, None)),
            ));
        }
        if let Some(token) = m.syncing {
            out.push(Effect::Reply(
                token,
                Answer::Sync(SyncAnswer::refused(ResponseError::UnknownMemberId)),
            ));
        }
    }

    /// The group has lost a member: a rebalance waiting on it may now finish, and a running group must start one.
    pub(super) fn rebalance_after_change(&mut self, now: Millis, out: &mut Vec<Effect>) {
        match self.state {
            State::Empty => {}
            State::PreparingRebalance => self.maybe_complete(now, out),
            State::CompletingRebalance | State::Stable => {
                self.begin_rebalance(now, false, out);
                self.maybe_complete(now, out);
            }
        }
    }
}
