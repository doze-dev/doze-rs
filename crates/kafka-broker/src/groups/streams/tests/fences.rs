//! STR-70 to STR-74: who may commit and fetch a streams group's offsets (`StreamsGroup.validateOffsetCommit`,
//! `validateOffsetFetch`).

use kafka_wire::ResponseError as E;
use kafka_wire::Uuid;

use super::super::fetch_refusal;
use super::{World, beat, id, ready_topics};
use crate::groups::machine::CommitRequest;

/// `in` is topic 1, the repartition topic 2, the changelog 3 (in the order `ready_topics` makes them).
fn commit(member: Option<&str>, generation: i32, partitions: &[(u128, i32)]) -> CommitRequest {
    CommitRequest {
        version: 9,
        member_id: member.map(id),
        instance: None,
        generation,
        transactional: false,
        partitions: partitions
            .iter()
            .map(|(t, p)| (Uuid::from_u128(*t), *p))
            .collect(),
    }
}

#[test]
fn an_admin_commit_goes_ahead_only_while_nobody_is_in_the_group() {
    let mut w = World::new(ready_topics(2));
    assert_eq!(
        w.group
            .commit_fence(&commit(None, -1, &[(1, 0)]), &w.topics),
        None
    );
    w.settle(&["a"]);
    assert_eq!(
        w.group
            .commit_fence(&commit(None, -1, &[(1, 0)]), &w.topics),
        Some(E::UnknownMemberId)
    );
    let mut txn = commit(None, -1, &[(1, 0)]);
    txn.transactional = true;
    assert_eq!(
        w.group.commit_fence(&txn, &w.topics),
        None,
        "a producer committing for a group it is not in"
    );
}

#[test]
fn a_member_commits_at_its_epoch_on_offset_commit_nine_or_later() {
    let mut w = World::new(ready_topics(2));
    let last = w.settle(&["a"]);
    let e = last["a"].epoch;
    assert_eq!(
        w.group
            .commit_fence(&commit(Some("a"), e, &[(1, 0)]), &w.topics),
        None
    );
    let mut old = commit(Some("a"), e, &[(1, 0)]);
    old.version = 8;
    assert_eq!(
        w.group.commit_fence(&old, &w.topics),
        Some(E::UnsupportedVersion)
    );
    assert_eq!(
        w.group
            .commit_fence(&commit(Some("a"), e + 1, &[(1, 0)]), &w.topics),
        Some(E::StaleMemberEpoch)
    );
    assert_eq!(
        w.group
            .commit_fence(&commit(Some("x"), e, &[(1, 0)]), &w.topics),
        Some(E::UnknownMemberId)
    );
}

/// At an older epoch, a commit names only partitions of active tasks the member has run since that epoch, of topics the
/// topology reads; one that names anything else fails whole.
#[test]
fn a_commit_at_an_older_epoch_is_let_through_for_tasks_held_since_then() {
    let mut w = World::new(ready_topics(2));
    let last = w.settle(&["a"]);
    let e = last["a"].epoch;
    // Move the epoch on without moving a's tasks: a second member that is then gone.
    w.send(&super::join("b"));
    w.send(&beat("b", -1, None));
    w.advance(1_000);
    let a = w.send(&beat("a", e, Some(w.held("a"))));
    assert!(a.epoch > e, "{a:?}");
    assert_eq!(
        w.group
            .commit_fence(&commit(Some("a"), e, &[(1, 0), (2, 1)]), &w.topics),
        None
    );
    // The changelog is no topic the topology reads; an epoch before the task was given is too old.
    assert_eq!(
        w.group
            .commit_fence(&commit(Some("a"), e, &[(3, 0)]), &w.topics),
        Some(E::StaleMemberEpoch)
    );
    assert_eq!(
        w.group
            .commit_fence(&commit(Some("a"), e - 1, &[(1, 0)]), &w.topics),
        Some(E::StaleMemberEpoch)
    );
}

#[test]
fn a_member_fetches_offsets_only_at_exactly_its_epoch() {
    let mut w = World::new(ready_topics(2));
    let last = w.settle(&["a"]);
    let e = last["a"].epoch;
    let view = w.group.view();
    assert_eq!(fetch_refusal(&view, None, -1), None);
    assert_eq!(fetch_refusal(&view, Some("a"), e), None);
    assert_eq!(
        fetch_refusal(&view, Some("a"), e - 1),
        Some(E::StaleMemberEpoch)
    );
    assert_eq!(fetch_refusal(&view, Some("x"), e), Some(E::UnknownMemberId));
}
