//! The notes of the classic machine that are not a rebalance beginning: who is named, a generation formed and settled, a
//! member dropped. Each only reads the machine and pushes a note; none changes a field.

use kafka_wire::ids::MemberId;

use super::{Effect, GroupMachine, Millis};
use crate::groups::consumer::protocol::decode_assignment;
use crate::groups::note::{
    CAPTURE, Cause, MEMBERS_KEPT, Named, Reason, What, Why, count, holds, say,
};

impl GroupMachine {
    /// The member `id` as a note names it, while it is still in the group.
    pub(super) fn who_is(&self, id: &MemberId, reason: Option<Reason>) -> Option<Named> {
        if !CAPTURE {
            return None;
        }
        self.members
            .get(id)
            .map(|m| Named::new(id, &m.client_id, reason).with_session(m.session_ms))
    }

    /// Why a rebalance began because members `gone` left or went silent (named before they were taken out).
    pub(super) fn why_gone(&self, cause: Cause, gone: Vec<Named>) -> Why {
        let more = gone.len().saturating_sub(1);
        let mut why = Why::new(cause, None, self.members.len() + gone.len());
        why.who = gone.into_iter().next();
        why.more = count(more);
        why
    }

    /// A member that did not join again before the rebalance ran out of time.
    pub(super) fn note_dropped(
        &self,
        id: &MemberId,
        began: Option<Millis>,
        now: Millis,
        out: &mut Vec<Effect>,
    ) {
        say(out, now, || {
            let who = self
                .who_is(id, None)
                .unwrap_or_else(|| Named::new(id, "", None));
            What::Dropped {
                generation: self.generation.wrapping_add(1).max(0),
                who,
                wait_ms: began.map_or(0, |b| now.0.saturating_sub(b.0)),
            }
        });
    }

    /// The members that joined, as the generation they formed.
    pub(super) fn note_formed(&self, now: Millis, out: &mut Vec<Effect>) {
        say(out, now, || What::Formed {
            generation: self.generation,
            members: self.members.keys().take(MEMBERS_KEPT).cloned().collect(),
            total: count(self.members.len()),
            leader: self.leader.clone(),
        });
    }

    /// The leader's assignment is out: what each member holds, as a consumer assignment says it.
    pub(super) fn note_settled(&self, now: Millis, out: &mut Vec<Effect>) {
        let consumers = self.protocol_type.as_deref() == Some("consumer");
        say(out, now, || {
            let held = self.members.values().filter(|_| consumers).filter_map(|m| {
                decode_assignment(&m.assignment).map(|by_name| (m.id.clone(), by_name))
            });
            let (holds, total) = holds(held);
            What::Settled {
                generation: self.generation,
                holds,
                total,
            }
        });
    }
}
