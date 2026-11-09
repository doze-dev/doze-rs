//! Who may fetch a consumer group's offsets: OffsetFetch v9 names the member that asks and the epoch it believes it is at.
//!
//! Read from Apache Kafka 4.3.1's source (`ConsumerGroup.validateOffsetFetch`); the recordings hold no such request.

use kafka_wire::ResponseError as E;

use super::ConsumerView;

/// Why a member may not fetch the offsets of a consumer group. Nothing is checked of a request that names no member and
/// no epoch (an admin tool, or an older client); nor of a classic group, or a group that is not there (the caller does
/// not ask).
///
/// A member that is not in the group is UNKNOWN_MEMBER_ID; one at another epoch than the group's record is
/// STALE_MEMBER_EPOCH, or ILLEGAL_GENERATION if it joined by the classic protocol (its epoch is its generation). A name
/// with no epoch (-1), or an epoch with no name, is a member too: of one that is not there, or at the wrong epoch.
pub(crate) fn fetch_refusal(view: &ConsumerView, member: Option<&str>, epoch: i32) -> Option<E> {
    if member.is_none() && epoch < 0 {
        return None;
    }
    let Some(m) = member.and_then(|id| view.members.iter().find(|m| m.id.as_str() == id)) else {
        return Some(E::UnknownMemberId);
    };
    if m.epoch == epoch {
        None
    } else if m.classic {
        Some(E::IllegalGeneration)
    } else {
        Some(E::StaleMemberEpoch)
    }
}
