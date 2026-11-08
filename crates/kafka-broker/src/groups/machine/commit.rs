//! Whether an OffsetCommit may be stored: the fence round the group's offsets.

use kafka_wire::ResponseError;

use super::{Answer, CommitRequest, Effect, GroupMachine, State, Token};

impl GroupMachine {
    /// Kafka's rules for a commit to a group that exists: with no generation (a simple consumer, or a tool resetting
    /// offsets) it is allowed only while the group has no members; otherwise the member must be in the group, at
    /// the group's generation, and the group must not be waiting for its leader's assignment.
    pub(super) fn commit(&self, token: Token, req: &CommitRequest, out: &mut Vec<Effect>) {
        out.push(Effect::Reply(token, Answer::Commit(self.commit_fence(req))));
    }

    fn commit_fence(&self, req: &CommitRequest) -> Option<ResponseError> {
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
        (self.state == State::CompletingRebalance).then_some(ResponseError::RebalanceInProgress)
    }
}

/// What a group that does not exist says to a commit: only a commit that names no generation may go ahead.
pub(crate) fn commit_to_no_group(generation: i32) -> Option<ResponseError> {
    (generation >= 0).then_some(ResponseError::IllegalGeneration)
}
