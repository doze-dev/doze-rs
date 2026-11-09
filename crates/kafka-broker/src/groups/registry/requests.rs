//! What the registry decides about a request before any group sees it.

use kafka_wire::ResponseError;

use crate::groups::actor::Req;
use crate::groups::consumer::HeartbeatAnswer;
use crate::groups::machine::{Answer, JoinAnswer, LeaveResult, SyncAnswer, commit_to_no_group};

/// Whether a request may make a group that is not there: a JoinGroup from a client with no id, or with one it was given.
pub(super) fn can_begin(req: &Req) -> bool {
    match req {
        Req::Join(j) => j.member_id.is_none() || (j.issued && j.instance.is_none()),
        // A consumer group is begun by a member joining it; one that keeps on, or leaves, a group that is not there
        // is told so.
        Req::ConsumerHeartbeat(h) => crate::groups::consumer::can_join(h),
        _ => false,
    }
}

/// What a group that does not exist says to anything but a join: it has no such member. (A request to describe or
/// delete one is not put at all.)
pub(super) fn unknown(req: &Req) -> Option<Answer> {
    Some(match req {
        Req::Join(_) => Answer::Join(JoinAnswer::refused(ResponseError::UnknownMemberId, None)),
        Req::Sync(_) => Answer::Sync(SyncAnswer::refused(ResponseError::UnknownMemberId)),
        Req::Heartbeat(_) => Answer::Heartbeat(Some(ResponseError::UnknownMemberId)),
        Req::Commit(c) => Answer::Commit(commit_to_no_group(c.generation)),
        Req::Leave(targets) => Answer::Leave(
            targets
                .iter()
                .map(|t| LeaveResult {
                    member_id: t.member_id.clone(),
                    instance: t.instance.clone(),
                    error: Some(ResponseError::UnknownMemberId),
                })
                .collect(),
        ),
        Req::ConsumerHeartbeat(h) => {
            let words = if h.epoch < 0 {
                format!("Group {} not found.", h.group)
            } else {
                format!("Consumer group {} not found.", h.group)
            };
            Answer::ConsumerHeartbeat(HeartbeatAnswer::refused(
                ResponseError::GroupIdNotFound,
                words,
            ))
        }
        Req::Describe | Req::Delete | Req::TopicsChanged => return None,
    })
}
