//! A classic member in a consumer group: what it is told, when it is removed, and what a refused request leaves alone.

use super::*;

fn heartbeat_of(r: &mut Rig, name: &str, generation: i32) -> Option<E> {
    let t = r.send(|t| {
        Event::Heartbeat(
            t,
            HeartbeatRequest {
                member_id: id(name),
                instance: None,
                generation,
            },
        )
    });
    let Answer::Heartbeat(e) = r.answer(t) else {
        panic!("not a heartbeat")
    };
    e
}

/// A classic member joins again with its id, as it does when told to; its generation now.
fn rejoin(r: &mut Rig, name: &str) -> i32 {
    let t = r.send(|t| Event::Join(t, join_request(Some(name))));
    let Answer::Join(j) = r.answer(t) else {
        panic!("not a join")
    };
    assert!(j.error.is_none(), "{:?}", j.error);
    j.generation
}

fn members_of(r: &mut Rig) -> Vec<String> {
    r.view()
        .map(|v| v.members.iter().map(|m| m.id.to_string()).collect())
        .unwrap_or_default()
}

#[test]
fn a_heartbeat_that_is_refused_changes_nothing_of_the_group() {
    for bad in ["bogus", ""] {
        let mut r = Rig::new();
        r.classic_member("old");
        let before = r.group.member_count();
        let t = r.send(|t| {
            let mut hb = heartbeat("new", 0);
            if bad == "bogus" {
                hb.assignor = Some("bogus".into());
            } else {
                hb.member_id = None;
            }
            Event::ConsumerHeartbeat(t, hb)
        });
        let Answer::ConsumerHeartbeat(a) = r.answer(t) else {
            panic!("not a heartbeat")
        };
        assert!(a.error.is_some(), "{bad:?}");
        assert!(r.view().is_none(), "{bad:?}: still a classic group");
        assert_eq!(r.group.member_count(), before);
        assert_eq!(
            heartbeat_of(&mut r, "old", 1),
            None,
            "and its member is as it was"
        );
    }
    // An empty classic group stays classic too.
    let mut r = Rig::new();
    let t = r.send(|t| {
        let mut hb = heartbeat("new", 0);
        hb.assignor = Some("bogus".into());
        Event::ConsumerHeartbeat(t, hb)
    });
    r.answer(t);
    assert!(r.view().is_none());
}

#[test]
fn a_classic_member_that_is_told_to_rejoin_and_never_does_is_removed_at_its_rebalance_timeout() {
    let mut r = Rig::new();
    let generation = r.classic_member("old");
    r.consumer("new", 0);
    // The consumer is targeted a share of the partitions "old" holds: "old" must rejoin to give some up.
    r.now += 1_500;
    let epoch = r
        .view()
        .unwrap()
        .members
        .iter()
        .find(|m| m.id.as_str() == "new")
        .unwrap()
        .epoch;
    r.consumer("new", epoch);
    assert_eq!(
        heartbeat_of(&mut r, "old", generation),
        Some(E::RebalanceInProgress)
    );
    // It heartbeats on every five seconds and never joins again: the thirty seconds it gave are up.
    for _ in 0..7 {
        r.advance(5_000);
        heartbeat_of(&mut r, "old", generation);
        r.consumer("new", epoch);
    }
    assert!(!members_of(&mut r).contains(&"old".to_owned()), "removed");
}

#[test]
fn a_classic_member_gets_the_partitions_it_may_have_now_at_the_version_it_subscribed_with() {
    let mut r = Rig::new();
    r.classic_member("old");
    r.consumer("new", 0);
    let generation = rejoin(&mut r, "old");
    let sync = r.send(|t| {
        Event::Sync(
            t,
            SyncRequest {
                member_id: id("old"),
                instance: None,
                generation,
                protocol_type: Some("consumer".into()),
                protocol: Some("range".into()),
                assignments: vec![],
            },
        )
    });
    let Answer::Sync(s) = r.answer(sync) else {
        panic!("not a sync")
    };
    assert!(s.error.is_none(), "{:?}", s.error);
    let held = crate::groups::consumer::protocol::decode_assignment(&s.assignment).unwrap();
    assert_eq!(
        held.get("t").map(Vec::len),
        Some(1),
        "its share, with the consumer taking the other"
    );
    assert_eq!(
        &s.assignment[..2],
        &[0, 0],
        "at version 0, the one it subscribed with"
    );
    // Another generation, another protocol type or name, another member: each in its own code.
    let ask = |r: &mut Rig, name: &str, g: i32, ty: Option<&str>| {
        let t = r.send(|t| {
            Event::Sync(
                t,
                SyncRequest {
                    member_id: id(name),
                    instance: None,
                    generation: g,
                    protocol_type: ty.map(str::to_owned),
                    protocol: None,
                    assignments: vec![],
                },
            )
        });
        let Answer::Sync(s) = r.answer(t) else {
            panic!("not a sync")
        };
        s.error
    };
    assert_eq!(
        ask(&mut r, "old", generation + 5, None),
        Some(E::IllegalGeneration)
    );
    assert_eq!(
        ask(&mut r, "old", generation, Some("connect")),
        Some(E::InconsistentGroupProtocol)
    );
    assert_eq!(
        ask(&mut r, "nobody", generation, None),
        Some(E::UnknownMemberId)
    );
}

#[test]
fn a_classic_leave_removes_a_member_of_either_protocol_and_the_group_stays_a_consumer_group() {
    let mut r = Rig::new();
    r.classic_member("old");
    r.consumer("new", 0);
    let leave = r.send(|t| {
        Event::Leave(
            t,
            vec![
                crate::groups::machine::LeaveTarget {
                    member_id: Some(id("new")),
                    instance: None,
                },
                crate::groups::machine::LeaveTarget {
                    member_id: Some(id("ghost")),
                    instance: None,
                },
            ],
        )
    });
    let Answer::Leave(results) = r.answer(leave) else {
        panic!("not a leave")
    };
    assert_eq!(
        results.iter().map(|x| x.error).collect::<Vec<_>>(),
        [None, Some(E::UnknownMemberId)]
    );
    assert_eq!(members_of(&mut r), ["old"]);
    assert!(r.view().is_some(), "not made classic again by this");
}

#[test]
fn a_member_that_runs_out_of_session_as_the_last_of_the_new_protocol_makes_the_group_classic() {
    let mut r = Rig::new();
    r.classic_member("old");
    r.consumer("new", 0);
    // "old" keeps heartbeating and joins again when told to (its session is ten seconds); "new" is silent for its forty-five.
    let mut generation = rejoin(&mut r, "old");
    for _ in 0..10 {
        r.advance(5_000);
        if heartbeat_of(&mut r, "old", generation) == Some(E::RebalanceInProgress) {
            generation = rejoin(&mut r, "old");
        }
    }
    assert!(r.view().is_none(), "classic again");
}

#[test]
fn a_member_subscribed_by_pattern_shows_the_topics_it_matches() {
    let mut r = Rig::new();
    let t = r.send(|t| {
        let mut hb = heartbeat("m", 0);
        hb.subscribed = Some(vec![]);
        hb.regex = Some("[tu]".into());
        Event::ConsumerHeartbeat(t, hb)
    });
    r.answer(t);
    let d = r.send(Event::Describe);
    let Answer::Describe(d) = r.answer(d) else {
        panic!("not a description")
    };
    assert_eq!(
        d.subscribed.map(|s| s.into_iter().collect::<Vec<_>>()),
        Some(vec!["t".to_owned(), "u".to_owned()])
    );
}

#[test]
fn a_group_of_another_protocol_type_is_not_upgraded() {
    let mut r = Rig::new();
    let mut connect = join_request(Some("w"));
    connect.protocol_type = "connect".into();
    let t = r.send(|t| Event::Join(t, connect));
    r.answer(t);
    let a = r.consumer("c", 0);
    let (error, words) = a.error.expect("refused");
    assert_eq!(error, E::GroupIdNotFound);
    assert!(
        words.starts_with("Cannot upgrade classic group g to consumer group because"),
        "{words}"
    );
}

#[test]
fn a_syncgroup_waiting_for_the_leader_is_answered_rebalance_in_progress_when_the_group_is_upgraded()
{
    let mut r = Rig::new();
    // A second member joins a running group and the first joins again: one new generation, in which the leader has
    // not synced, so the follower's SyncGroup is parked.
    r.classic_member("a");
    let b = r.send(|t| Event::Join(t, join_request(Some("b"))));
    let a = r.send(|t| Event::Join(t, join_request(Some("a"))));
    let (Answer::Join(ja), Answer::Join(_)) = (r.answer(a), r.answer(b)) else {
        panic!("not joins")
    };
    let sync = r.send(|t| {
        Event::Sync(
            t,
            SyncRequest {
                member_id: id("b"),
                instance: None,
                generation: ja.generation,
                protocol_type: None,
                protocol: None,
                assignments: vec![],
            },
        )
    });
    assert!(r.answers.is_empty(), "parked");
    assert!(r.consumer("c", 0).error.is_none());
    let Answer::Sync(s) = r.answer(sync) else {
        panic!("not a sync")
    };
    assert_eq!(s.error, Some(E::RebalanceInProgress));
}

#[test]
fn a_classic_member_of_a_consumer_group_is_fenced_by_its_generation() {
    let mut r = Rig::new();
    let generation = r.classic_member("old");
    r.consumer("new", 0);
    let generation = rejoin(&mut r, "old").max(generation);
    assert_eq!(
        heartbeat_of(&mut r, "old", generation + 7),
        Some(E::IllegalGeneration)
    );
    assert_eq!(
        heartbeat_of(&mut r, "nobody", generation),
        Some(E::UnknownMemberId)
    );
}

#[test]
fn the_generation_after_a_downgrade_is_beyond_every_epoch_the_members_held() {
    let mut r = Rig::new();
    r.classic_member("old");
    r.consumer("new", 0);
    let held = rejoin(&mut r, "old");
    r.consumer("new", -1);
    assert!(r.view().is_none());
    let next = rejoin(&mut r, "old");
    assert!(next > held, "{next} after {held}");
}

#[test]
fn a_consumer_group_takes_a_thousand_members_and_no_more() {
    let mut r = Rig::new();
    for i in 0..1_000 {
        let a = r.consumer(&format!("m{i}"), 0);
        assert!(a.error.is_none(), "member {i}: {:?}", a.error);
    }
    let a = r.consumer("one-too-many", 0);
    assert_eq!(a.error.map(|(e, _)| e), Some(E::GroupMaxSizeReached));
    // One that is already a member may join again.
    assert!(r.consumer("m7", 0).error.is_none());
}
