//! The notes of a consumer group: its epoch moved, and why. Each only reads the group and pushes a note.

use kafka_wire::ids::MemberId;

use super::ConsumerGroup;
use crate::groups::machine::{Effect, Millis};
use crate::groups::note::{CAPTURE, Cause, Named, Reason, What, Why, count, say};

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
    pub(super) fn bumped(
        &self,
        before: i32,
        cause: Cause,
        who: Option<Named>,
        members_before: usize,
        now: Millis,
        out: &mut Vec<Effect>,
    ) {
        if self.epoch == before {
            return;
        }
        say(out, now, || What::Epoch {
            why: Why::new(cause, who, members_before),
            epoch: self.epoch,
            after: count(self.members.len()),
        });
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
