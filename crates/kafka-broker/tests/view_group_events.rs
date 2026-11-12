//! `ConsoleView::group_events`: what a group has said of itself, through a real broker and the requests a client sends.
#![cfg(feature = "inspect")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::Duration;

use kafka_auth::Principal;
use kafka_broker::{Caller, Cause, EventKind, GroupEvent};
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::leave_group_request::MemberIdentity;
use kafka_wire::messages::{GroupId, LeaveGroupRequest};
use support::acls::{admin, closed_world, user};
use support::{Rig, join_request, rig_with};

fn console() -> Caller {
    Caller::new(Principal::console(), Some("127.0.0.1".parse().unwrap()))
}

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

fn instant() -> Rig {
    rig_with(|c| c.group_initial_rebalance_delay = Duration::ZERO)
}

/// A JoinGroup v5 with a reason: the id the broker hands out first, then the join that takes it. The member id.
async fn join(rig: &Rig, group: &str, reason: &str) -> String {
    let mut req = join_request(group, "", None, &[("range", "m")]);
    req.reason = Some(text(reason));
    let first = rig.join(5, req.clone()).await;
    assert_eq!(first.error_code, 79, "a v5 client is given an id first");
    req.member_id = first.member_id.clone();
    let joined = rig.join(5, req).await;
    assert_eq!(joined.error_code, 0);
    joined.member_id.as_str().to_owned()
}

async fn leave(rig: &Rig, group: &str, member: &str, reason: &str) {
    let mut req = LeaveGroupRequest::default();
    req.group_id = GroupId(text(group));
    let mut who = MemberIdentity::default();
    who.member_id = text(member);
    who.reason = Some(text(reason));
    req.members = vec![who];
    let Response::LeaveGroup(r) = rig.ask(5, Request::LeaveGroup(req)).await else {
        panic!("not LeaveGroup")
    };
    assert_eq!(r.members[0].error_code, 0);
}

async fn events(rig: &Rig, group: &str) -> Vec<GroupEvent> {
    rig.broker
        .console_view()
        .group_events(&console(), group)
        .await
        .expect("the group is in memory")
        .events
}

#[tokio::test]
async fn a_join_and_a_leave_are_told_with_their_causes_and_the_clients_clipped_reasons() {
    let rig = instant();
    let huge = "r".repeat(900 * 1024);
    let member = join(&rig, "story", &huge).await;
    leave(&rig, "story", &member, &huge).await;
    let seen = events(&rig, "story").await;
    let kinds: Vec<_> = seen.iter().map(|e| (e.kind, e.cause)).collect();
    assert_eq!(
        kinds,
        [
            (EventKind::Began, Some(Cause::MemberJoined)),
            (EventKind::Formed, None),
            (EventKind::Began, Some(Cause::MemberLeft)),
            (EventKind::Formed, None),
        ]
    );
    assert_eq!(seen[0].member.as_deref(), Some(member.as_str()));
    assert_eq!(seen[0].generation, 1);
    assert_eq!(
        seen[0].reason.as_ref().map(String::len),
        Some(128),
        "clipped"
    );
    assert_eq!(
        seen[2].reason.as_ref().map(String::len),
        Some(128),
        "clipped"
    );
    assert_eq!(
        (seen[2].members_before, seen[2].members_after),
        (Some(1), Some(0))
    );
    assert_eq!(seen[3].members_total, 0, "the group is Empty");
    assert!(seen[0].at_ms <= seen[3].at_ms);
}

#[tokio::test]
async fn a_look_changes_nothing_and_says_the_same_twice() {
    let rig = instant();
    join(&rig, "story", "starting").await;
    let first = events(&rig, "story").await;
    let detail = rig
        .broker
        .console_view()
        .group_detail(&console(), "story")
        .await
        .unwrap();
    let mut again = events(&rig, "story").await;
    // An event's time is the wall clock of the look minus how long ago the group's own task noted it, so two looks may
    // differ by a millisecond of rounding between the two clocks: everything else must be identical.
    assert_eq!(first.len(), again.len());
    for (a, b) in first.iter().zip(again.iter_mut()) {
        assert!((a.at_ms - b.at_ms).abs() <= 5, "{} vs {}", a.at_ms, b.at_ms);
        b.at_ms = a.at_ms;
    }
    assert_eq!(first, again);
    let after = rig
        .broker
        .console_view()
        .group_detail(&console(), "story")
        .await
        .unwrap();
    assert_eq!(
        (detail.generation, detail.members.len()),
        (after.generation, after.members.len())
    );
}

#[tokio::test]
async fn a_group_the_broker_does_not_have_has_no_story_to_tell() {
    let rig = instant();
    let view = rig.broker.console_view();
    assert!(view.group_events(&console(), "nobody").await.is_none());
    // Hostile ids are no group at all.
    for hostile in ["", "../../etc", &"g".repeat(300), "a\nb", "<script>"] {
        assert!(
            view.group_events(&console(), hostile).await.is_none(),
            "{hostile:?}"
        );
    }
}

#[tokio::test]
async fn a_caller_who_may_not_describe_the_group_is_told_nothing_and_not_that_it_exists() {
    let rig = rig_with(|c| {
        closed_world(c);
        c.group_initial_rebalance_delay = Duration::ZERO;
    });
    let mut req = join_request("secret", "", None, &[("range", "m")]);
    req.reason = Some(text("hi"));
    let Response::JoinGroup(first) = rig
        .ask_as(&admin(), 4, Request::JoinGroup(req.clone()))
        .await
    else {
        panic!("not JoinGroup")
    };
    req.member_id = first.member_id;
    rig.ask_as(&admin(), 4, Request::JoinGroup(req)).await;
    let view = rig.broker.console_view();
    assert!(view.group_events(&user("bob"), "secret").await.is_none());
    assert!(view.group_events(&user("bob"), "absent").await.is_none());
    assert!(view.group_events(&console(), "secret").await.is_some());
}

#[tokio::test]
async fn a_member_that_goes_silent_is_a_session_that_expired() {
    let rig = instant();
    let member = join(&rig, "quiet", "x").await;
    // The session (10 s) and then the rebalance have to run out; the group's task keeps its own clock.
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(11)).await;
    // The first heartbeat after is refused: the member is gone.
    let r = rig.heartbeat("quiet", &member, 1).await;
    assert_ne!(r.error_code, 0);
    let seen = events(&rig, "quiet").await;
    assert!(
        seen.iter().any(|e| e.cause == Some(Cause::SessionExpired)),
        "{seen:?}"
    );
}

#[tokio::test]
async fn partitions_added_to_a_topic_a_consumer_group_reads_are_a_topic_that_changed() {
    let rig = instant();
    rig.create(vec![support::topic("orders", 2)], false).await;
    let joined = rig
        .heartbeat_848(
            1,
            support::join_848("fleet", "member-a-00000000000000000000", &["orders"]),
        )
        .await;
    assert_eq!(joined.error_code, 0, "{:?}", joined.error_message);
    let result = rig
        .create_partitions(3, false, &[("orders", 5, None)])
        .await;
    assert_eq!(result[0].1, 0, "{result:?}");
    // The groups are told of the change in their own time: look until it has arrived.
    let mut seen = Vec::new();
    for _ in 0..50 {
        seen = events(&rig, "fleet").await;
        if seen.iter().any(|e| e.cause == Some(Cause::TopicChanged)) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let causes: Vec<_> = seen
        .iter()
        .map(|e| (e.kind, e.cause, e.generation))
        .collect();
    assert_eq!(
        causes,
        [
            (EventKind::Epoch, Some(Cause::MemberJoined), 2),
            (EventKind::Epoch, Some(Cause::TopicChanged), 3),
        ]
    );
    assert_eq!(
        seen[0].member.as_deref(),
        Some("member-a-00000000000000000000")
    );
    assert!(seen[1].member.is_none(), "no member did it");
    assert_eq!(
        (seen[1].members_before, seen[1].members_after),
        (Some(1), Some(1))
    );
}
