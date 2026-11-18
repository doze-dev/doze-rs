//! One StreamsGroupHeartbeat, in Kafka 4.4.0's order (`GroupMetadataManager.streamsGroupHeartbeat`): who it is from, the
//! topology, what the member says of itself, the topology against the topics, the group epoch, the targets, the member's
//! step towards its own, and the answer. Everything that can refuse the request is decided before anything changes: Kafka
//! writes nothing for a request it refuses.

use std::collections::BTreeMap;

use kafka_wire::ResponseError as E;

use super::member::Member;
use super::reconcile::{Holders, Stage, reconcile};
use super::seat::Seat;
use super::topology::{Configured, MISSING_INTERNAL_TOPICS, Topology, configure};
use super::{StreamsAnswer, StreamsGroup, StreamsHeartbeat, Task, Tasks, endpoints, status};
use crate::groups::consumer::Topics;
use crate::groups::machine::{Answer, Effect, Millis, TimerKind, Token};
use crate::groups::note::Cause;

/// What a heartbeat that is not refused is to change, worked out before anything is.
struct Plan {
    seat: Seat,
    /// The topology to set, for a group that has none.
    topology: Option<Topology>,
    /// The topology against the topics, when it is to be worked out again, and the topics it was worked out against.
    configured: Option<(Configured, BTreeMap<String, (kafka_wire::Uuid, i32)>)>,
    offsets: Option<BTreeMap<Task, i64>>,
    end_offsets: Option<BTreeMap<Task, i64>>,
}

/// Task offsets by task; one task twice is refused.
fn offsets(list: Option<&Vec<(Task, i64)>>) -> Result<Option<BTreeMap<Task, i64>>, StreamsAnswer> {
    let Some(list) = list else { return Ok(None) };
    let mut out = BTreeMap::new();
    for ((sub, p), offset) in list {
        if out.insert((sub.clone(), *p), *offset).is_some() {
            return Err(StreamsAnswer::refused(
                E::InvalidRequest,
                format!("Task offsets contain a duplicate entry for subtopology {sub} and partition {p}."),
            ));
        }
    }
    Ok(Some(out))
}

/// The tasks a member says it owns must be tasks of the topology (checked only when the topology is ready).
fn check_owned(owned: &Tasks, configured: &Configured) -> Result<(), StreamsAnswer> {
    for (sub, p) in owned.all() {
        let Some(s) = configured.subtopologies.get(sub) else {
            return Err(StreamsAnswer::refused(
                E::InvalidRequest,
                format!("Subtopology {sub} does not exist in the topology."),
            ));
        };
        if *p < 0 || *p >= s.tasks {
            return Err(StreamsAnswer::refused(
                E::InvalidRequest,
                format!(
                    "Task {p} for subtopology {sub} is invalid. Number of tasks for this subtopology: {}",
                    s.tasks
                ),
            ));
        }
    }
    Ok(())
}

impl StreamsGroup {
    /// Handles one StreamsGroupHeartbeat.
    pub(crate) fn heartbeat(
        &mut self,
        token: Token,
        hb: &StreamsHeartbeat,
        topics: &Topics,
        now: Millis,
        out: &mut Vec<Effect>,
    ) {
        let answer = if hb.epoch < 0 {
            self.leave(hb, now, out)
        } else {
            self.plan(hb, topics)
                .map_or_else(|refused| refused, |plan| self.beat(hb, plan, topics, now, out))
        };
        out.push(Effect::Reply(token, Answer::StreamsHeartbeat(answer)));
    }

    /// The topology a join brings: set if the group has none; one at an older epoch than the group's is let be (the
    /// member is told its topology is stale); any other that differs is refused, as Kafka 4.4.0 updates none.
    fn topology_to_set(&self, hb: &StreamsHeartbeat) -> Result<Option<Topology>, StreamsAnswer> {
        match (&self.topology, &hb.topology) {
            (None, Some(t)) => Ok(Some(t.clone())),
            (Some(g), Some(t)) if g.epoch <= t.epoch && g != t => Err(StreamsAnswer::refused(
                E::InvalidRequest,
                "Topology updates are not supported yet.",
            )),
            (None, None) => Err(StreamsAnswer::refused(
                E::UnknownServerError,
                format!("The streams group {} has no topology.", hb.group),
            )),
            _ => Ok(None),
        }
    }

    fn plan(&self, hb: &StreamsHeartbeat, topics: &Topics) -> Result<Plan, StreamsAnswer> {
        let seat = self.seat_of(hb)?;
        let to_set = self.topology_to_set(hb)?;
        let Some(topology) = to_set.as_ref().or(self.topology.as_ref()) else {
            return Err(StreamsAnswer::refused(E::UnknownServerError, "no topology"));
        };
        let seen = topology.seen(topics);
        let configured = if self.configured.is_none() || to_set.is_some() || seen != self.seen {
            let c = configure(topology, topics).map_err(|invalid| {
                let code = if invalid.internal {
                    E::UnknownServerError
                } else {
                    E::StreamsInvalidTopology
                };
                StreamsAnswer::refused(code, invalid.words)
            })?;
            Some((c, seen))
        } else {
            None
        };
        let current = configured.as_ref().map(|c| &c.0).or(self.configured.as_ref());
        if let (Some(c), Some(owned)) = (current.filter(|c| c.ready()), &hb.owned) {
            check_owned(owned, c)?;
        }
        Ok(Plan {
            seat,
            topology: to_set,
            configured,
            offsets: offsets(hb.offsets.as_ref())?,
            end_offsets: offsets(hb.end_offsets.as_ref())?,
        })
    }

    /// Seats the member the plan says, and says whether it is new to the group.
    fn take_seat(&mut self, hb: &StreamsHeartbeat, seat: Seat, out: &mut Vec<Effect>) -> bool {
        let id = &hb.member_id;
        match seat {
            Seat::Known => false,
            Seat::New => {
                self.members.insert(id.clone(), Member::new(id.clone()));
                true
            }
            Seat::Replaces(old) => {
                out.push(Effect::Disarm(TimerKind::Session(old.clone())));
                out.push(Effect::Disarm(TimerKind::Revoke(old.clone())));
                if let Some(mut m) = self.members.remove(&old) {
                    m.id = id.clone();
                    m.held.epoch = 0;
                    m.held.prev_epoch = 0;
                    self.members.insert(id.clone(), m);
                }
                if let Some(t) = self.targets.remove(&old) {
                    self.targets.insert(id.clone(), t);
                }
                false
            }
        }
    }

    /// Applies the plan's changes to the group; `Some(cause)` if the group epoch is to move.
    fn apply(&mut self, hb: &StreamsHeartbeat, plan: Plan, out: &mut Vec<Effect>) -> (Option<Cause>, bool) {
        let joined = self.take_seat(hb, plan.seat, out);
        if plan.topology.is_some() {
            self.topology = plan.topology;
        }
        let mut cause = joined.then_some(Cause::MemberJoined);
        let mut endpoint_moved = false;
        if let Some(m) = self.members.get_mut(&hb.member_id) {
            let changed = m.update(hb);
            endpoint_moved = changed.endpoint;
            if changed.group {
                cause = cause.or(Some(Cause::SubscriptionChanged));
            }
            if let Some(o) = plan.offsets {
                m.offsets = o;
            }
            if let Some(o) = plan.end_offsets {
                m.end_offsets = o;
            }
        }
        if let Some((configured, seen)) = plan.configured {
            if seen != self.seen {
                cause = cause.or(Some(Cause::TopicChanged));
            }
            self.seen = seen;
            self.configured = Some(configured);
        }
        let validated = match (&self.configured, &self.topology) {
            (Some(c), Some(t)) if c.ready() => t.epoch,
            _ => -1,
        };
        if validated != self.validated_epoch {
            self.validated_epoch = validated;
            cause = cause.or(Some(Cause::TopicChanged));
        }
        let configs = Some((hb.timing.standbys, hb.timing.tags.clone()));
        if configs != self.assignment_configs {
            self.assignment_configs = configs;
            cause = cause.or(Some(Cause::SubscriptionChanged));
        }
        self.assign_interval_ms = hb.timing.assign_interval_ms;
        (cause, endpoint_moved)
    }

    fn beat(
        &mut self,
        hb: &StreamsHeartbeat,
        plan: Plan,
        topics: &Topics,
        now: Millis,
        out: &mut Vec<Effect>,
    ) -> StreamsAnswer {
        // Kafka returns endpoint epoch 0 to the heartbeat that made the group (its epoch is not written yet).
        let new_group = self.topology.is_none() && self.members.is_empty();
        let was_empty = self.members.is_empty();
        let was = self.members.len();
        let (cause, endpoint_moved) = self.apply(hb, plan, out);
        if let Some(cause) = cause {
            self.epoch += 1;
            let who = self.who_is(&hb.member_id);
            self.note_epoch(cause, who, was, now, out);
        }
        if was_empty && hb.timing.initial_delay_ms > 0 && !self.delaying {
            self.delaying = true;
            self.arm(TimerKind::Hold, now.plus(hb.timing.initial_delay_ms), out);
        }
        let (target_epoch, target, delayed) = self.target_of(&hb.member_id, now);
        let moved = self.step_member(hb, target_epoch, &target, now, out);
        if hb.shutdown && self.shutdown_by.is_none() {
            self.shutdown_by = Some(hb.member_id.clone());
        }
        let Some(m) = self.members.get(&hb.member_id) else {
            return StreamsAnswer::refused(E::UnknownServerError, "the member is gone");
        };
        if endpoint_moved || (moved && m.endpoint.is_some()) {
            self.endpoint_epoch += 1;
        }
        let answer = self.answer(hb, delayed, moved, new_group, topics);
        // The heartbeat that makes a group bumps an endpoint epoch Kafka does not keep (the group is made again from its
        // records): the next heartbeat finds it at 0.
        if new_group {
            self.endpoint_epoch = 0;
        }
        answer
    }

    /// The member's target and its epoch, the targets computed first if they may be; and whether that was delayed.
    fn target_of(&mut self, id: &kafka_wire::ids::MemberId, now: Millis) -> (i32, Tasks, Option<&'static str>) {
        if self.delaying {
            return (
                self.assignment_epoch,
                Tasks::default(),
                Some("Assignment delayed due to the configured initial rebalance delay."),
            );
        }
        let mut delayed = None;
        if self.epoch > self.assignment_epoch {
            if self.may_assign(now) {
                self.compute_targets(now);
            } else {
                delayed = Some("Assignment delayed due to the configured assignment interval.");
            }
        }
        let target = self.targets.get(id).cloned().unwrap_or_default();
        (self.assignment_epoch, target, delayed)
    }

    pub(super) fn may_assign(&self, now: Millis) -> bool {
        self.assigned_at
            .is_none_or(|at| self.assign_interval_ms == 0 || now.0 >= at.0.saturating_add(self.assign_interval_ms))
    }

    /// The member's step towards its target, its session started again, and its rebalance timer as its stage says.
    /// `true`: the tasks it holds changed.
    fn step_member(
        &mut self,
        hb: &StreamsHeartbeat,
        target_epoch: i32,
        target: &Tasks,
        now: Millis,
        out: &mut Vec<Effect>,
    ) -> bool {
        let id = &hb.member_id;
        let next = {
            let mut holders = Holders::default();
            for m in self.members.values() {
                holders.add(&m.process, &m.held);
            }
            self.members
                .get(id)
                .and_then(|m| reconcile(&m.held, &m.process, target, target_epoch, hb.owned.as_ref(), &holders))
        };
        let session = self.arm(TimerKind::Session(id.clone()), now.plus(hb.timing.session_ms), out);
        let revoke = next
            .as_ref()
            .filter(|n| n.stage == Stage::Unrevoked)
            .zip(self.members.get(id))
            .map(|(n, m)| (n.epoch, now.plus(u64::try_from(m.rebalance_ms).unwrap_or(0))));
        let revoke_gen = match (&next, revoke) {
            (_, Some((_, at))) => Some(self.arm(TimerKind::Revoke(id.clone()), at, out)),
            (Some(_), None) => {
                out.push(Effect::Disarm(TimerKind::Revoke(id.clone())));
                None
            }
            (None, None) => None,
        };
        let Some(m) = self.members.get_mut(id) else { return false };
        m.session_gen = session;
        m.session_ms = hb.timing.session_ms;
        m.heard = now;
        if let Some((gen_, epoch)) = revoke_gen.zip(revoke.map(|r| r.0)) {
            (m.revoke_gen, m.revoke_epoch) = (gen_, epoch);
        }
        let Some(n) = next else { return false };
        let moved = n.tasks != m.held.tasks;
        m.held = n;
        moved
    }
}

impl StreamsGroup {
    fn answer(
        &self,
        hb: &StreamsHeartbeat,
        delayed: Option<&'static str>,
        moved: bool,
        new_group: bool,
        topics: &Topics,
    ) -> StreamsAnswer {
        let Some(m) = self.members.get(&hb.member_id) else {
            return StreamsAnswer::refused(E::UnknownServerError, "the member is gone");
        };
        let endpoints = (self.endpoint_epoch != hb.endpoint_epoch).then(|| {
            self.topology.as_ref().map_or_else(Vec::new, |t| {
                endpoints::build(self.members.values(), m, t, topics)
            })
        });
        let create = match self.configured.as_ref().and_then(|c| c.not_ready.as_ref()) {
            Some(n) if n.code == MISSING_INTERNAL_TOPICS => {
                self.configured.as_ref().map(|c| c.create.clone()).unwrap_or_default()
            }
            _ => Vec::new(),
        };
        StreamsAnswer {
            error: None,
            member_id: Some(m.id.clone()),
            epoch: m.held.epoch,
            heartbeat_ms: hb.timing.heartbeat_ms,
            task_offset_ms: hb.timing.task_offset_ms,
            acceptable_lag: hb.timing.acceptable_lag,
            status: self.statuses(hb, m, delayed),
            tasks: (hb.epoch == 0 || moved).then(|| m.held.tasks.clone()),
            endpoint_epoch: if new_group { 0 } else { self.endpoint_epoch },
            endpoints,
            create,
        }
    }

    /// The statuses, in Kafka's order.
    fn statuses(&self, hb: &StreamsHeartbeat, m: &Member, delayed: Option<&'static str>) -> Vec<(i8, String)> {
        let mut out = Vec::new();
        if let Some(t) = self.topology.as_ref().filter(|t| m.topology_epoch < t.epoch) {
            out.push((
                status::STALE_TOPOLOGY,
                format!(
                    "The member's topology epoch {} is behind the group's topology epoch {}.",
                    m.topology_epoch, t.epoch
                ),
            ));
        }
        if let Some(words) = delayed {
            out.push((status::ASSIGNMENT_DELAYED, words.to_owned()));
        }
        if let Some(n) = self.configured.as_ref().and_then(|c| c.not_ready.as_ref()) {
            out.push((n.code, n.detail.clone()));
        }
        if let Some(by) = &self.shutdown_by {
            out.push((
                status::SHUTDOWN_APPLICATION,
                format!(
                    "Streams group member {by} encountered a fatal error and requested a shutdown for the entire application."
                ),
            ));
        }
        let missing: Vec<&str> = hb
            .timing
            .tags
            .iter()
            .map(String::as_str)
            .filter(|t| !m.client_tags.contains_key(*t))
            .collect();
        if hb.version >= 1 && !missing.is_empty() {
            out.push((
                status::MISSING_CLIENT_TAGS,
                format!(
                    "Missing required client tags for rack-aware standby assignment: [{}]. Configure them via 'client.tag.<tagKey>' in your Streams config.",
                    missing.join(", ")
                ),
            ));
        }
        out
    }
}
