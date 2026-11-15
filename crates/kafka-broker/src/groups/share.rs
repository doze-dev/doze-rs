//! A share group (KIP-932): members heartbeat to say which topics they read, and the broker spreads the partitions over
//! them, sharing a partition between members when there are more members than partitions. There is nothing to revoke: a
//! partition may be read by several members at once, each record going to one of them at a time (the share-partitions
//! keep that, in `crate::share`), so a member's assignment is its target as soon as the target is computed.
//!
//! The rules are Kafka 4.4.0's `GroupMetadataManager.shareGroupHeartbeat` and its helpers: the group epoch moves with the
//! members, their subscriptions and the subscribed topics' partitions; the targets are computed for an epoch by the first
//! heartbeat at least the assignment interval after the last computation; a member is told its epoch and, when it changed,
//! its assignment. Pure, like the consumer group: the actor hands in the time and the topics.

pub(crate) mod assign;
mod seat;
#[cfg(test)]
mod tests;
mod view;

use std::collections::{BTreeMap, BTreeSet};

use kafka_wire::ResponseError as E;
use kafka_wire::Uuid;
use kafka_wire::ids::MemberId;

use self::assign::Subscribed;
use super::consumer::Topics;
use super::consumer::assign::Partitions;
use super::machine::{Answer, Effect, Millis, TimerKey, TimerKind, Token};
use super::note::{CAPTURE, Cause, Named, What, Why, count, say};

/// The timings of a share group: its configs, or the broker's defaults, as the heartbeat finds them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShareTiming {
    /// `share.session.timeout.ms`.
    pub(crate) session_ms: u64,
    /// `share.heartbeat.interval.ms`.
    pub(crate) heartbeat_ms: i32,
    /// `share.assignment.interval.ms`.
    pub(crate) assign_interval_ms: u64,
    /// `group.share.max.size`.
    pub(crate) max_size: usize,
}

/// A ShareGroupHeartbeat, after the checks that need nothing but the request.
#[derive(Debug, Clone)]
pub(crate) struct ShareHeartbeat {
    pub(crate) group: String,
    pub(crate) member_id: MemberId,
    /// 0 to join, -1 to leave, else the epoch the member is at.
    pub(crate) epoch: i32,
    /// Sent when it changes; `None`: unchanged.
    pub(crate) rack: Option<String>,
    /// Sent when it changes; `None`: unchanged.
    pub(crate) subscribed: Option<Vec<String>>,
    pub(crate) client_id: String,
    pub(crate) client_host: String,
    pub(crate) timing: ShareTiming,
}

/// The answer to a ShareGroupHeartbeat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShareAnswer {
    pub(crate) error: Option<(E, String)>,
    pub(crate) member_id: Option<MemberId>,
    pub(crate) epoch: i32,
    pub(crate) heartbeat_ms: i32,
    /// What the member is to read, when it joined, said what it subscribes to, or that changed.
    pub(crate) assignment: Option<Partitions>,
    /// The topics the group reads, and its epoch, when a member joined or the epoch moved: the share-partitions of
    /// their partitions are to be made if they are not there yet (Kafka's InitializeShareGroupState).
    pub(crate) initialize: Option<(i32, BTreeSet<String>)>,
}

impl ShareAnswer {
    pub(crate) fn refused(error: E, words: impl Into<String>) -> Self {
        Self {
            error: Some((error, words.into())),
            member_id: None,
            epoch: 0,
            heartbeat_ms: 0,
            assignment: None,
            initialize: None,
        }
    }
}

/// A member as ShareGroupDescribe shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShareMemberView {
    pub(crate) id: MemberId,
    pub(crate) rack: Option<String>,
    pub(crate) epoch: i32,
    pub(crate) client_id: String,
    pub(crate) client_host: String,
    pub(crate) subscribed: Vec<String>,
    pub(crate) assigned: Partitions,
}

/// A share group as ShareGroupDescribe and ListGroups show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShareView {
    /// `Empty` or `Stable`: Kafka sets no other.
    pub(crate) state: &'static str,
    pub(crate) epoch: i32,
    pub(crate) assignment_epoch: i32,
    pub(crate) members: Vec<ShareMemberView>,
}

struct Member {
    id: MemberId,
    epoch: i32,
    prev_epoch: i32,
    rack: Option<String>,
    client_id: String,
    client_host: String,
    subscribed: BTreeSet<String>,
    assigned: Partitions,
    session_gen: u64,
    session_ms: u64,
    /// When its session timer last started; only the console reads it.
    #[cfg_attr(not(feature = "inspect"), allow(dead_code))]
    heard: Millis,
}

/// A share group.
pub(crate) struct ShareGroup {
    epoch: i32,
    assignment_epoch: i32,
    /// When the targets were last computed; `None` (Kafka's timestamp 0): at the next heartbeat whatever the interval.
    assigned_at: Option<Millis>,
    members: BTreeMap<MemberId, Member>,
    targets: BTreeMap<MemberId, Partitions>,
    /// The subscribed topics as the targets were last computed for (Kafka's metadata hash): id and partition count.
    seen: BTreeMap<String, (Uuid, i32)>,
    next_arming: u64,
}

pub(super) fn not_a_member(member: &str, group: &str) -> String {
    format!("Member {member} is not a member of group {group}.")
}

impl ShareGroup {
    /// A group nobody has joined.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::restored(0, 0)
    }

    /// An empty group as it was when the broker last stopped.
    #[must_use]
    pub(crate) fn restored(epoch: i32, assignment_epoch: i32) -> Self {
        Self {
            epoch,
            assignment_epoch,
            assigned_at: None,
            members: BTreeMap::new(),
            targets: BTreeMap::new(),
            seen: BTreeMap::new(),
            next_arming: 0,
        }
    }

    #[must_use]
    pub(crate) fn member_count(&self) -> usize {
        self.members.len()
    }

    /// The epochs, for the record kept across a restart.
    #[must_use]
    pub(crate) fn epochs(&self) -> (i32, i32) {
        (self.epoch, self.assignment_epoch)
    }

    /// Whether this heartbeat would seat a member who is not in the group now.
    #[must_use]
    pub(crate) fn would_seat(&self, hb: &ShareHeartbeat) -> bool {
        hb.epoch == 0
            && !self.members.contains_key(&hb.member_id)
            && self.members.len() < hb.timing.max_size
    }

    /// The topics the members read.
    fn subscribed(&self) -> BTreeSet<String> {
        self.members
            .values()
            .flat_map(|m| m.subscribed.iter().cloned())
            .collect()
    }

    /// Handles one ShareGroupHeartbeat.
    pub(crate) fn heartbeat(
        &mut self,
        token: Token,
        hb: &ShareHeartbeat,
        topics: &Topics,
        now: Millis,
        out: &mut Vec<Effect>,
    ) {
        let answer = if hb.epoch == -1 {
            self.leave(hb, now, out)
        } else {
            self.beat(hb, topics, now, out)
        };
        out.push(Effect::Reply(token, Answer::ShareHeartbeat(answer)));
    }

    fn leave(&mut self, hb: &ShareHeartbeat, now: Millis, out: &mut Vec<Effect>) -> ShareAnswer {
        if !self.members.contains_key(&hb.member_id) {
            return ShareAnswer::refused(
                E::UnknownMemberId,
                not_a_member(hb.member_id.as_str(), &hb.group),
            );
        }
        self.remove(&hb.member_id, Cause::MemberLeft, now, out);
        ShareAnswer {
            error: None,
            member_id: Some(hb.member_id.clone()),
            epoch: -1,
            heartbeat_ms: 0,
            assignment: None,
            initialize: None,
        }
    }

    /// A member goes: the epoch moves, and if it was the last, the (empty) targets are for the new epoch at once.
    fn remove(&mut self, id: &MemberId, cause: Cause, now: Millis, out: &mut Vec<Effect>) {
        let was = self.members.len();
        let who = self.who_is(id);
        self.members.remove(id);
        self.targets.remove(id);
        out.push(Effect::Disarm(TimerKind::Session(id.clone())));
        self.epoch += 1;
        if self.members.is_empty() {
            self.assignment_epoch = self.epoch;
            self.assigned_at = None;
        }
        self.note(Why::new(cause, who, was), now, out);
    }

    fn beat(
        &mut self,
        hb: &ShareHeartbeat,
        topics: &Topics,
        now: Millis,
        out: &mut Vec<Effect>,
    ) -> ShareAnswer {
        if let Some(refused) = self.refusal(hb) {
            return refused;
        }
        let id = &hb.member_id;
        let was = self.members.len();
        let joined = !self.members.contains_key(id);
        let subscription_changed = self.seat(hb, now);
        let before = self.epoch;
        let cause = if joined {
            Some(Cause::MemberJoined)
        } else if subscription_changed {
            Some(Cause::SubscriptionChanged)
        } else if self.topics_moved(topics) {
            Some(Cause::TopicChanged)
        } else {
            None
        };
        if cause.is_some() || subscription_changed {
            self.epoch += 1;
            self.seen = self.seen_now(topics);
        }
        if self.assignment_epoch < self.epoch
            && self
                .assigned_at
                .is_none_or(|at| now.0.saturating_sub(at.0) >= hb.timing.assign_interval_ms)
        {
            self.compute_targets(topics, now);
        }
        let answer_assignment = self.reconcile(id, subscription_changed, topics);
        self.reset_session(id, now, out);
        if let Some(cause) = cause {
            let who = self.who_is(id);
            self.note(Why::new(cause, who, was), now, out);
        }
        let Some(m) = self.members.get(id) else {
            return ShareAnswer::refused(E::UnknownMemberId, not_a_member(id.as_str(), &hb.group));
        };
        let tell = hb.epoch == 0 || hb.subscribed.is_some() || answer_assignment;
        ShareAnswer {
            error: None,
            member_id: Some(id.clone()),
            epoch: m.epoch,
            heartbeat_ms: hb.timing.heartbeat_ms,
            assignment: tell.then(|| m.assigned.clone()),
            initialize: (joined || self.epoch != before).then(|| (self.epoch, self.subscribed())),
        }
    }

    /// The member catches up with the targets: its epoch is theirs and it holds its target (there is nothing to give up
    /// first). `true`: what it holds changed.
    fn reconcile(&mut self, id: &MemberId, subscription_changed: bool, topics: &Topics) -> bool {
        let target_epoch = self.assignment_epoch;
        let target = self.targets.get(id).cloned().unwrap_or_default();
        let Some(m) = self.members.get_mut(id) else {
            return false;
        };
        let wanted: BTreeSet<Uuid> = m
            .subscribed
            .iter()
            .filter_map(|n| topics.id_of(n))
            .collect();
        if m.epoch != target_epoch {
            m.prev_epoch = m.epoch;
            m.epoch = target_epoch;
            let assigned: Partitions = target
                .into_iter()
                .filter(|p| wanted.contains(&p.0))
                .collect();
            let changed = assigned != m.assigned;
            m.assigned = assigned;
            return changed;
        }
        if subscription_changed {
            let before = m.assigned.len();
            m.assigned.retain(|p| wanted.contains(&p.0));
            return before != m.assigned.len();
        }
        false
    }

    /// The subscribed topics, by name, with their id and partition count now.
    fn seen_now(&self, topics: &Topics) -> BTreeMap<String, (Uuid, i32)> {
        self.subscribed()
            .into_iter()
            .filter_map(|name| {
                topics
                    .all()
                    .find(|t| t.name == name)
                    .map(|t| (name, (t.id, t.partitions)))
            })
            .collect()
    }

    /// The subscribed topics are not what the targets were computed for: made, deleted, grown.
    fn topics_moved(&self, topics: &Topics) -> bool {
        self.seen != self.seen_now(topics)
    }

    fn compute_targets(&mut self, topics: &Topics, now: Millis) {
        let members: BTreeSet<MemberId> = self.members.keys().cloned().collect();
        let mut subscribed: BTreeMap<String, Subscribed> = BTreeMap::new();
        for m in self.members.values() {
            for name in &m.subscribed {
                let Some(t) = topics.all().find(|t| &t.name == name) else {
                    continue;
                };
                subscribed
                    .entry(name.clone())
                    .or_insert_with(|| Subscribed {
                        id: t.id,
                        partitions: t.partitions,
                        subscribers: BTreeSet::new(),
                    })
                    .subscribers
                    .insert(m.id.clone());
            }
        }
        self.targets = assign::assign(&members, &subscribed, &self.targets);
        self.assignment_epoch = self.epoch;
        self.assigned_at = Some(now);
    }

    fn reset_session(&mut self, id: &MemberId, now: Millis, out: &mut Vec<Effect>) {
        let session = self.members.get(id).map_or(0, |m| m.session_ms);
        self.next_arming += 1;
        let generation = self.next_arming;
        out.push(Effect::Arm {
            key: TimerKey {
                kind: TimerKind::Session(id.clone()),
                generation,
            },
            at: now.plus(session),
        });
        if let Some(m) = self.members.get_mut(id) {
            m.session_gen = generation;
            m.heard = now;
        }
    }

    /// A timer ran out: a member's session, if it was not started again since.
    pub(crate) fn timer(&mut self, key: &TimerKey, now: Millis, out: &mut Vec<Effect>) {
        if let TimerKind::Session(id) = &key.kind
            && self
                .members
                .get(id)
                .is_some_and(|m| m.session_gen == key.generation)
        {
            let id = id.clone();
            self.remove(&id, Cause::SessionExpired, now, out);
        }
    }

    /// A topic was made, grown or deleted: the next heartbeat looks.
    pub(crate) fn topics_changed(&mut self) {}

    /// The Lab's steps: a member's session runs out, or the epoch moves (the next heartbeat computes new targets).
    pub(crate) fn force_expire(
        &mut self,
        id: &MemberId,
        now: Millis,
        out: &mut Vec<Effect>,
    ) -> bool {
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
        self.note(Why::new(Cause::Forced, None, self.members.len()), now, out);
        true
    }

    fn who_is(&self, id: &MemberId) -> Option<Named> {
        if !CAPTURE {
            return None;
        }
        self.members
            .get(id)
            .map(|m| Named::new(id, &m.client_id, None).with_session(m.session_ms))
    }

    fn note(&self, why: Why, now: Millis, out: &mut Vec<Effect>) {
        say(out, now, || What::Epoch {
            why,
            epoch: self.epoch,
            after: count(self.members.len()),
        });
    }
}
