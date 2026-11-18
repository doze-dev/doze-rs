//! Who may commit and fetch a streams group's offsets (Kafka 4.4.0's `StreamsGroup.validateOffsetCommit` and
//! `validateOffsetFetch`).

use kafka_wire::ResponseError as E;

use super::{StreamsGroup, StreamsView};
use crate::groups::consumer::Topics;
use crate::groups::machine::CommitRequest;

impl StreamsGroup {
    /// The subtopology whose tasks read `topic`.
    fn subtopology_reading(&self, topic: &str) -> Option<&str> {
        self.topology.as_ref().and_then(|t| {
            t.subtopologies
                .iter()
                .find(|(_, s)| s.inputs().any(|i| i == topic))
                .map(|(id, _)| id.as_str())
        })
    }

    /// Why a commit may not be stored, if it may not: with nobody in the group, one that names no epoch goes ahead (an
    /// admin tool); a transaction's that names nobody does too. Otherwise the member must be there, on OffsetCommit v9 or
    /// later, at its own epoch, or at an older one under which it already ran every partition the commit names as an
    /// active task (held or being given up). One partition that fails fails the request.
    pub(crate) fn commit_fence(&self, req: &CommitRequest, topics: &Topics) -> Option<E> {
        if req.generation < 0 && self.members.is_empty() {
            return None;
        }
        if req.transactional && req.names_nobody() {
            return None;
        }
        let Some(member) = req.member_id.as_ref().and_then(|id| self.members.get(id)) else {
            return Some(E::UnknownMemberId);
        };
        if !req.transactional && req.version < 9 {
            return Some(E::UnsupportedVersion);
        }
        let epoch = member.held.epoch;
        if req.generation == epoch {
            return None;
        }
        let refused = if req.transactional {
            E::IllegalGeneration
        } else {
            E::StaleMemberEpoch
        };
        if req.generation > epoch {
            return Some(refused);
        }
        let held = req.partitions.iter().all(|(id, p)| {
            let task = topics
                .name_of(id)
                .and_then(|name| self.subtopology_reading(name))
                .map(|sub| (sub.to_owned(), *p));
            task.and_then(|t| member.held.given_at.get(&t))
                .is_some_and(|given| *given <= req.generation)
        });
        (!held).then_some(refused)
    }
}

/// Why a member may not fetch a streams group's offsets: nothing is checked of a request that names no member and no
/// epoch; otherwise the member must be there, at exactly its epoch.
pub(crate) fn fetch_refusal(view: &StreamsView, member: Option<&str>, epoch: i32) -> Option<E> {
    if member.is_none() && epoch < 0 {
        return None;
    }
    let m = member.and_then(|id| view.members.iter().find(|m| m.id.as_str() == id));
    match m {
        None => Some(E::UnknownMemberId),
        Some(m) if m.epoch != epoch => Some(E::StaleMemberEpoch),
        Some(_) => None,
    }
}
