//! What the classic machine says of itself: each way a rebalance begins has its cause, and what comes of it is said too.
//! (That saying it changes nothing is `group::tests::differential`.)

use super::*;
use crate::groups::consumer::protocol::{ByName, encode_assignment};
use crate::groups::note::{Cause, Note, Reason, What};

/// The rebalances that began, as (cause, generation, members before, members after).
fn began(g: &Group) -> Vec<(Cause, i32, u32, u32)> {
    g.notes
        .iter()
        .filter_map(|n| match &n.what {
            What::Began {
                why,
                generation,
                after,
                ..
            } => Some((why.cause, *generation, why.before, *after)),
            _ => None,
        })
        .collect()
}

fn who(note: &Note) -> Option<(String, String, Option<String>)> {
    match &note.what {
        What::Began { why, .. } => why.who.as_ref(),
        What::Dropped { who, .. } => Some(who),
        _ => None,
    }
    .map(|w| {
        (
            w.member.to_string(),
            w.client.to_string(),
            w.reason.as_ref().map(|r| r.as_str().to_owned()),
        )
    })
}

#[test]
fn the_first_member_of_an_empty_group_begins_generation_one_and_it_is_said_who() {
    let mut g = Group::new(0);
    let mut req = join_request(None, &["range"]);
    req.reason = Reason::clip("consumer started");
    g.join(req);
    assert_eq!(began(&g), [(Cause::MemberJoined, 1, 0, 1)]);
    assert_eq!(
        who(&g.notes[0]),
        Some(("m1".into(), "test".into(), Some("consumer started".into())))
    );
    let What::Formed {
        generation,
        members,
        total,
        leader,
    } = &g.notes[1].what
    else {
        panic!("the generation is formed next: {:?}", g.notes[1])
    };
    assert_eq!(
        (
            *generation,
            members.len(),
            *total,
            leader.as_ref().map(|l| l.as_str())
        ),
        (1, 1, 1, Some("m1"))
    );
}

#[test]
fn a_member_joining_a_running_group_is_a_rebalance_for_that_reason() {
    let mut g = stable(2);
    g.notes.clear();
    g.join(join_request(None, &["range"]));
    assert_eq!(began(&g), [(Cause::MemberJoined, 2, 2, 3)]);
}

#[test]
fn a_second_member_joining_a_rebalance_already_forming_is_not_a_new_one() {
    let mut g = stable(2);
    g.notes.clear();
    g.join(join_request(None, &["range"]));
    g.join(join_request(None, &["range"]));
    assert_eq!(
        began(&g).len(),
        1,
        "one generation is forming, for the first reason"
    );
}

#[test]
fn a_leave_is_a_rebalance_for_that_reason_with_the_clients_own_words() {
    let mut g = stable(3);
    g.notes.clear();
    let targets = vec![LeaveTarget {
        member_id: Some(id("m2")),
        instance: None,
        reason: Reason::clip("shutting down for a deploy"),
    }];
    g.send(|t| Event::Leave(t, targets));
    assert_eq!(began(&g), [(Cause::MemberLeft, 2, 3, 2)]);
    assert_eq!(
        who(&g.notes[0]),
        Some((
            "m2".into(),
            "test".into(),
            Some("shutting down for a deploy".into())
        ))
    );
}

#[test]
fn two_members_leaving_together_are_one_rebalance_naming_the_first_and_counting_the_other() {
    let mut g = stable(3);
    g.notes.clear();
    g.leave(&["m2", "m3"]);
    assert_eq!(began(&g), [(Cause::MemberLeft, 2, 3, 1)]);
    let What::Began { why, .. } = &g.notes[0].what else {
        panic!("not a rebalance")
    };
    assert_eq!(why.more, 1);
}

#[test]
fn a_member_that_sends_no_heartbeat_is_a_session_that_expired_not_a_leave() {
    let mut g = stable(2);
    g.notes.clear();
    g.advance(11_000);
    let began = began(&g);
    assert_eq!(began.len(), 1, "{:?}", g.notes);
    assert_eq!(began[0].0, Cause::SessionExpired);
    assert_eq!((began[0].2, began[0].3), (2, 1));
}

#[test]
fn a_member_that_joins_again_with_other_protocols_says_so() {
    let mut g = stable(2);
    g.notes.clear();
    g.join(join_request(Some("m2"), &["range", "roundrobin"]));
    assert_eq!(began(&g), [(Cause::ProtocolsChanged, 2, 2, 2)]);
}

#[test]
fn the_leader_joining_a_running_group_again_with_what_it_had_is_the_leader_rejoining() {
    let mut g = stable(2);
    g.notes.clear();
    g.join(join_request(Some("m1"), &["range"]));
    assert_eq!(began(&g), [(Cause::LeaderRejoined, 2, 2, 2)]);
}

#[test]
fn a_follower_joining_again_with_what_it_had_is_no_rebalance_and_no_note() {
    let mut g = stable(2);
    g.notes.clear();
    g.join(join_request(Some("m2"), &["range"]));
    assert!(g.notes.is_empty(), "{:?}", g.notes);
}

#[test]
fn a_member_that_does_not_join_again_in_time_is_dropped_as_the_generation_forms() {
    let mut g = stable(2);
    g.notes.clear();
    // m1 joins again, m2 never does: the rebalance timeout (30 s) ends the wait.
    g.join(join_request(Some("m1"), &["range", "other"]));
    // m2 is heard from (so its session holds) but does not join again.
    for _ in 0..6 {
        g.advance(5_000);
        g.heartbeat("m2", 1);
    }
    g.advance(2_000);
    let dropped: Vec<_> = g
        .notes
        .iter()
        .filter(|n| matches!(n.what, What::Dropped { .. }))
        .collect();
    assert_eq!(dropped.len(), 1, "{:?}", g.notes);
    assert_eq!(who(dropped[0]).unwrap().0, "m2");
    let What::Dropped { wait_ms, .. } = dropped[0].what else {
        panic!()
    };
    assert_eq!(
        wait_ms, 30_000,
        "it was waited for as long as the members were given"
    );
    let formed = g
        .notes
        .iter()
        .find_map(|n| match &n.what {
            What::Formed { total, .. } => Some(*total),
            _ => None,
        })
        .unwrap();
    assert_eq!(formed, 1);
}

#[test]
fn the_group_that_empties_says_a_generation_formed_with_nobody_in_it() {
    let mut g = stable(1);
    g.notes.clear();
    g.leave(&["m1"]);
    assert!(
        g.notes
            .iter()
            .any(|n| matches!(&n.what, What::Formed { total: 0, .. })),
        "{:?}",
        g.notes
    );
}

#[test]
fn the_leaders_assignment_handed_out_is_said_as_what_each_member_holds() {
    let mut g = Group::new(100);
    let tokens: Vec<_> = (0..2)
        .map(|_| g.join(join_request(None, &["range"])))
        .collect();
    g.advance(500);
    for t in tokens {
        g.joined(t);
    }
    let held = |parts: &[i32]| {
        encode_assignment(0, &ByName::from([("orders".to_owned(), parts.to_vec())]))
    };
    let req = SyncRequest {
        member_id: id("m1"),
        instance: None,
        generation: 1,
        protocol_type: None,
        protocol: None,
        assignments: vec![(id("m1"), held(&[0, 1])), (id("m2"), held(&[2]))],
    };
    g.send(|t| Event::Sync(t, req));
    let What::Settled {
        generation,
        holds,
        total,
    } = &g.notes.last().unwrap().what
    else {
        panic!("the assignment is out: {:?}", g.notes.last())
    };
    assert_eq!((*generation, *total, holds.len()), (1, 2, 2));
    assert_eq!(holds[0].member.as_str(), "m1");
    assert_eq!(holds[0].topics[0].1, [0, 1]);
    assert_eq!(holds[1].total, 1);
}

#[test]
fn an_assignment_that_is_not_the_consumer_protocols_is_not_read() {
    let mut g = Group::new(0);
    let t = g.join(join_request(None, &["range"]));
    g.joined(t);
    // Not an assignment the consumer protocol could decode: the group is Stable all the same, and says no holds.
    g.sync("m1", 1, &[("m1", "opaque")]);
    let What::Settled { holds, .. } = &g.notes.last().unwrap().what else {
        panic!("settled: {:?}", g.notes.last())
    };
    assert!(holds.is_empty());
}
