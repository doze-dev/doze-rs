//! A member's right to DESCRIBE topics, as the requests of its principal bring it, and what follows when it changes.
//!
//! Kafka checks DESCRIBE on the names a member sends, once, at the request, and on what an expression matches, with the
//! principal that scheduled the resolution (`filterTopicDescribeAuthorizedTopics`). Neither covers the other sources of
//! a member's topics: the names and the assignment a static member leaves behind for the principal that takes its place,
//! a right taken away after the names were sent, a classic member's subscription. Here the member keeps the rights of
//! the latest request of its principal (`Member::rights`), every source is held to them (`Member::visible_names`, `Member::pattern_takes`), and when
//! they change what the member holds is withdrawn before the request is answered (SEC-230 to SEC-233).

use std::collections::HashSet;

use kafka_wire::Uuid;
use kafka_wire::ids::MemberId;

use super::member::Member;
use super::{ConsumerGroup, TopicInfo, Topics};
use crate::authorizer::{TopicRights, permits};
use crate::groups::machine::{Effect, Millis};

/// May a caller with these rights (`None`: unnarrowed) DESCRIBE the topic with this id? A topic that is gone reveals
/// nothing and is let through for the usual handling of a topic that has gone.
fn allowed(rights: Option<&TopicRights>, topics: &Topics, id: &Uuid) -> bool {
    rights.is_none() || topics.name_of(id).is_none_or(|n| permits(rights, n))
}

impl Member {
    /// Whether the member has any partition of this topic as a target, owned or to give up.
    fn holds(&self, id: Uuid) -> bool {
        let range = (id, i32::MIN)..=(id, i32::MAX);
        [&self.target, &self.assigned, &self.revoking]
            .iter()
            .any(|set| set.range(range.clone()).next().is_some())
    }

    /// Whether the topic is one the member subscribes to, matches or holds.
    fn concerns(&self, topic: &TopicInfo, names: &HashSet<&str>) -> bool {
        names.contains(topic.name.as_str())
            || self
                .pattern
                .as_ref()
                .is_some_and(|p| p.matches(&topic.name))
            || self.holds(topic.id)
    }

    /// Takes the rights of the request just heard. `true` if they change the answer for a topic the member subscribes to
    /// or matches or holds.
    pub(super) fn take_rights(&mut self, new: Option<TopicRights>, topics: &Topics) -> bool {
        if self.rights == new {
            return false;
        }
        let old = std::mem::replace(&mut self.rights, new);
        let names: HashSet<&str> = self.subscribed.iter().map(String::as_str).collect();
        topics.all().any(|t| {
            self.concerns(t, &names)
                && permits(old.as_ref(), &t.name) != permits(self.rights.as_ref(), &t.name)
        })
    }

    /// Takes from the member's target at once what it may not DESCRIBE, and moves what it owns of that into what it
    /// is to give up: it is not told of it again. `true` if it owned some.
    pub(super) fn withdraw(&mut self, topics: &Topics) -> bool {
        let Self {
            rights,
            target,
            assigned,
            revoking,
            ..
        } = self;
        let rights = rights.as_ref();
        target.retain(|(t, _)| allowed(rights, topics, t));
        let (keep, hidden): (_, super::assign::Partitions) = std::mem::take(assigned)
            .into_iter()
            .partition(|(t, _)| allowed(rights, topics, t));
        *assigned = keep;
        let moved = !hidden.is_empty();
        revoking.extend(hidden);
        moved
    }

    /// Drops from the member's target and from what it owns whatever it may not DESCRIBE, without a handover: for a
    /// member whose holdings are only a record (a classic member adopted by a consumer group).
    pub(super) fn forget_hidden(&mut self, topics: &Topics) {
        let rights = self.rights.as_ref();
        self.target.retain(|(t, _)| allowed(rights, topics, t));
        self.assigned.retain(|(t, _)| allowed(rights, topics, t));
    }
}

impl ConsumerGroup {
    /// Gives the member the rights of the request just heard. If they change what it may have, what it holds of the
    /// rest is withdrawn now (and the time to give it up starts), whether or not the targets are due to be computed.
    /// `true` if the group's epoch is to move.
    pub(super) fn refresh_rights(
        &mut self,
        id: &MemberId,
        new: Option<TopicRights>,
        topics: &Topics,
        now: Millis,
        out: &mut Vec<Effect>,
    ) -> bool {
        let Some(m) = self.members.get_mut(id) else {
            return false;
        };
        if !m.take_rights(new, topics) {
            return false;
        }
        if m.withdraw(topics) && m.revoke_gen == 0 {
            self.arm_revoke(id, now, out);
        }
        true
    }
}
