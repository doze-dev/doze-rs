//! Whether an OffsetCommit may be stored: the fence round the group's offsets.

use kafka_wire::ResponseError;

use kafka_wire::ids::{InstanceId, MemberId};

use super::{Answer, Effect, GroupMachine, State, Token};

/// An OffsetCommit, as far as the group is concerned: may it be stored?
#[derive(Debug, Clone)]
pub(crate) struct CommitRequest {
    /// The version of the OffsetCommit.
    pub(crate) version: i16,
    /// Who commits, if it says.
    pub(crate) member_id: Option<MemberId>,
    /// Its static instance id, if it has one.
    pub(crate) instance: Option<InstanceId>,
    /// The generation it believes it is in; negative if it names none.
    pub(crate) generation: i32,
    /// A transaction's offsets (TxnOffsetCommit): the producer need not be a member at all, and a group waiting for its
    /// leader's assignment does not refuse them.
    pub(crate) transactional: bool,
}

impl CommitRequest {
    /// It names no generation, no member and no instance: a producer committing offsets for a group it is not in.
    pub(crate) fn names_nobody(&self) -> bool {
        self.generation < 0 && self.member_id.is_none() && self.instance.is_none()
    }
}

impl GroupMachine {
    /// Kafka's rules for a commit to a group that exists: with no generation (a simple consumer, or a tool resetting
    /// offsets) it is allowed only while the group has no members; otherwise the member must be in the group, at
    /// the group's generation, and the group must not be waiting for its leader's assignment.
    pub(super) fn commit(&self, token: Token, req: &CommitRequest, out: &mut Vec<Effect>) {
        out.push(Effect::Reply(token, Answer::Commit(self.commit_fence(req))));
    }

    fn commit_fence(&self, req: &CommitRequest) -> Option<ResponseError> {
        if req.transactional && req.names_nobody() {
            return None;
        }
        if req.generation < 0 && self.members.is_empty() {
            return None;
        }
        let Some(member) = &req.member_id else {
            return Some(ResponseError::UnknownMemberId);
        };
        if let Err(e) = self.fenced(member, req.instance.as_ref()) {
            return Some(e);
        }
        if req.generation != self.generation {
            return Some(ResponseError::IllegalGeneration);
        }
        (!req.transactional && self.state == State::CompletingRebalance)
            .then_some(ResponseError::RebalanceInProgress)
    }
}

/// What a group that does not exist says to a commit: only a commit that names no generation may go ahead.
pub(crate) fn commit_to_no_group(generation: i32) -> Option<ResponseError> {
    (generation >= 0).then_some(ResponseError::IllegalGeneration)
}
