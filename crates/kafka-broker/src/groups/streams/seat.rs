//! Who the heartbeat is from (Kafka 4.4.0's `getOrMaybeCreate*StreamsGroupMember` and
//! `throwIfStreamsGroupMemberEpochIsInvalid`), and members leaving: by id, or for a static member by its instance id.

use kafka_wire::ResponseError as E;
use kafka_wire::ids::MemberId;

use super::{Refusal, StreamsAnswer, StreamsGroup, StreamsHeartbeat, not_a_member};
use crate::groups::machine::{Effect, Millis, TimerKind};
use crate::groups::note::Cause;

/// Where the heartbeat's member is to come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Seat {
    /// It is in the group.
    Known,
    /// It joins as a new member.
    New,
    /// It takes the place of the static member that left (epoch -2) with its instance id.
    Replaces(MemberId),
}

fn fenced(word: &str, received: i32, known: i32) -> Refusal {
    (
        E::FencedMemberEpoch,
        format!(
            "The streams group member has a {word} member epoch ({received}) than the one known by the group coordinator ({known}). The member must abandon all its partitions and rejoin."
        ),
    )
}

impl StreamsGroup {
    /// Where the member of `hb` comes from, or why it may not heartbeat. Changes nothing.
    pub(super) fn seat_of(&self, hb: &StreamsHeartbeat) -> Result<Seat, Refusal> {
        let id = &hb.member_id;
        let holder = hb.instance.as_ref().and_then(|i| {
            self.members
                .values()
                .find(|m| m.instance.as_ref() == Some(i))
        });
        if hb.epoch == 0 {
            if !self.has_room(hb) && !self.members.contains_key(id) {
                return Err((
                    E::GroupMaxSizeReached,
                    format!(
                        "The streams group has reached its maximum capacity of {} members.",
                        hb.timing.max_size
                    ),
                ));
            }
            return match (hb.instance.as_ref(), holder) {
                (Some(instance), Some(h)) if h.held.epoch != -2 => Err((
                    E::UnreleasedInstanceId,
                    format!(
                        "Static member {id} with instance id {instance} cannot join the group because the instance id is owned by {} member.",
                        h.id
                    ),
                )),
                (Some(_), Some(h)) => Ok(Seat::Replaces(h.id.clone())),
                _ if self.members.contains_key(id) => Ok(Seat::Known),
                _ => Ok(Seat::New),
            };
        }
        let member = match (hb.instance.as_ref(), holder) {
            (Some(instance), None) => {
                return Err((
                    E::UnknownMemberId,
                    format!("Instance id {instance} is unknown."),
                ));
            }
            (Some(instance), Some(h)) if &h.id != id => {
                return Err((
                    E::FencedInstanceId,
                    format!("Static member {id} with instance id {instance} was fenced by member {}.", h.id),
                ));
            }
            (_, Some(h)) => h,
            (None, None) => self.members.get(id).ok_or_else(|| {
                (E::UnknownMemberId, not_a_member(id.as_str(), &hb.group))
            })?,
        };
        let known = member.held.epoch;
        if hb.epoch > known {
            return Err(fenced("greater", hb.epoch, known));
        }
        let retry = hb.epoch == member.held.prev_epoch
            && hb.owned.as_ref().is_some_and(|o| o.within(&member.held.tasks));
        if hb.epoch < known && !retry {
            return Err(fenced("smaller", hb.epoch, known));
        }
        Ok(Seat::Known)
    }

    /// A heartbeat at epoch -1 or -2.
    pub(super) fn leave(&mut self, hb: &StreamsHeartbeat, now: Millis, out: &mut Vec<Effect>) -> StreamsAnswer {
        let id = &hb.member_id;
        let holder = hb.instance.as_ref().map(|i| {
            self.members
                .values()
                .find(|m| m.instance.as_ref() == Some(i))
                .map(|m| m.id.clone())
        });
        let refused = match (&hb.instance, holder) {
            (Some(i), Some(None)) => Some((E::UnknownMemberId, format!("Instance id {i} is unknown."))),
            (Some(i), Some(Some(h))) if &h != id => Some((
                E::FencedInstanceId,
                format!("Static member {id} with instance id {i} was fenced by member {h}."),
            )),
            (None, _) if !self.members.contains_key(id) => {
                Some((E::UnknownMemberId, not_a_member(id.as_str(), &hb.group)))
            }
            _ => None,
        };
        if let Some((error, words)) = refused {
            return StreamsAnswer::refused(error, words);
        }
        if hb.shutdown && self.shutdown_by.is_none() {
            self.shutdown_by = Some(id.clone());
        }
        if hb.epoch == -2 {
            // A static member leaves for now: its place is held, its session still running.
            if let Some(m) = self.members.get_mut(id) {
                m.held.epoch = -2;
                m.held.revoking = super::Tasks::default();
                for e in m.held.given_at.values_mut() {
                    *e = 0;
                }
            }
        } else {
            self.remove(id, Cause::MemberLeft, now, out);
        }
        StreamsAnswer {
            member_id: Some(id.clone()),
            epoch: hb.epoch,
            // The field's default: Kafka sets it on the answers to members that stay.
            acceptable_lag: -1,
            ..StreamsAnswer::default()
        }
    }

    /// A member goes: the epoch moves, and if it was the last, the (empty) targets are for the new epoch at once.
    pub(super) fn remove(&mut self, id: &MemberId, cause: Cause, now: Millis, out: &mut Vec<Effect>) {
        let was = self.members.len();
        let who = self.who_is(id);
        if self.members.remove(id).is_none() {
            return;
        }
        self.targets.remove(id);
        out.push(Effect::Disarm(TimerKind::Session(id.clone())));
        out.push(Effect::Disarm(TimerKind::Revoke(id.clone())));
        self.epoch += 1;
        if self.members.is_empty() {
            self.assignment_epoch = self.epoch;
            self.assigned_at = None;
            self.shutdown_by = None;
        }
        self.note_epoch(cause, who, was, now, out);
    }
}
