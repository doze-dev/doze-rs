//! The notes of a consumer group: its epoch moved, and why. Each only reads the group and pushes a note.

use kafka_wire::ids::MemberId;

use super::{ConsumerGroup, Topics};
use crate::authorizer::TopicRights;
use crate::groups::machine::{Effect, Millis};
use crate::groups::note::{CAPTURE, Cause, Named, Reason, What, Why, count, say};

/// What a join or a heartbeat found to change in the group, for the note of the epoch it moves.
pub(super) struct Moves {
    /// Something changed that moves the epoch.
    pub(super) changed: bool,
    /// The first reason it did.
    pub(super) cause: Option<Cause>,
    /// How many members the group had before the request.
    pub(super) was: usize,
    /// What the client said, if it said.
    pub(super) reason: Option<Reason>,
    /// What the caller may DESCRIBE, as the request brought it.
    pub(super) rights: Option<TopicRights>,
}

impl ConsumerGroup {
    /// The member `id` as a note names it, while it is still in the group.
    pub(super) fn who_is(&self, id: &MemberId, reason: Option<Reason>) -> Option<Named> {
        if !CAPTURE {
            return None;
        }
        self.members.get(id).map(|m| {
            let session = m
                .classic
                .as_ref()
                .map_or(self.cfg.session_ms, |c| c.session_ms);
            Named::new(id, &m.client_id, reason).with_session(session)
        })
    }

    /// The group's epoch was `before` and may have moved: if it did, say why.
    pub(super) fn bumped(&self, before: i32, why: Why, now: Millis, out: &mut Vec<Effect>) {
        if self.epoch == before {
            return;
        }
        say(out, now, || What::Epoch {
            why,
            epoch: self.epoch,
            after: count(self.members.len()),
        });
    }

    /// A member has joined or heartbeated: a right to DESCRIBE given or taken since last time, or the principal being another
    /// than the one that left the names and the partitions, changes what the member may have, and what it holds of the rest
    /// goes now (SEC-231). Then its session starts again, and the epoch moves if something changed, or the targets are
    /// computed if it is time.
    pub(super) fn settle(
        &mut self,
        id: &MemberId,
        mut moves: Moves,
        topics: &Topics,
        now: Millis,
        out: &mut Vec<Effect>,
    ) {
        let rights = self.refresh_rights(id, moves.rights.take(), topics, now, out);
        if rights {
            moves.cause.get_or_insert(Cause::RightsChanged);
        }
        self.reset_session(id, now, out);
        if moves.changed || rights {
            let who = self.who_is(id, moves.reason.take());
            let before = self.epoch;
            self.bump_and_reassign(topics, now);
            let cause = moves.cause.unwrap_or(Cause::MemberJoined);
            self.bumped(before, Why::new(cause, who, moves.was), now, out);
        } else {
            self.assign_if_due(topics, now);
        }
    }

    /// A classic group of consumers became this group, its members staying.
    pub(super) fn note_adopted(&self, members_before: usize, now: Millis, out: &mut Vec<Effect>) {
        say(out, now, || What::Epoch {
            why: Why::new(Cause::BecameConsumer, None, members_before),
            epoch: self.epoch,
            after: count(self.members.len()),
        });
    }
}
