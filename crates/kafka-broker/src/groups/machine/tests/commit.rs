//! The fence round a group's committed offsets.

use kafka_wire::ResponseError as E;

use super::*;

#[test]
fn a_member_of_the_current_generation_may_commit() {
    let mut g = stable(2);
    assert_eq!(g.commit(Some("m2"), 1), None);
}

#[test]
fn a_commit_names_a_member_that_is_there_then_the_generation_then_whether_the_group_is_waiting_for_its_leader()
 {
    let mut g = stable(2);
    assert_eq!(g.commit(Some("ghost"), 1), Some(E::UnknownMemberId));
    assert_eq!(g.commit(None, 1), Some(E::UnknownMemberId));
    assert_eq!(g.commit(Some("m1"), 0), Some(E::IllegalGeneration));
    assert_eq!(g.commit(Some("m1"), 2), Some(E::IllegalGeneration));
    // While the leader has not yet said who does what, nobody commits...
    let mut g = Group::new(100);
    let a = g.join(join_request(None, &["range"]));
    g.advance(500);
    g.joined(a);
    assert_eq!(g.commit(Some("m1"), 1), Some(E::RebalanceInProgress));
    // ...but members that are about to join again may (a rebalance under way is not a reason to lose progress).
    let mut g = stable(2);
    g.join(join_request(Some("m1"), &["range"]));
    assert_eq!(g.machine.state(), State::PreparingRebalance);
    assert_eq!(g.commit(Some("m2"), 1), None);
}

#[test]
fn a_commit_that_names_no_generation_is_allowed_only_while_the_group_has_no_members() {
    let mut g = stable(1);
    assert_eq!(g.commit(None, -1), Some(E::UnknownMemberId));
    assert_eq!(g.commit(Some("m1"), -1), Some(E::IllegalGeneration));
    g.leave(&["m1"]);
    assert_eq!(
        g.commit(None, -1),
        None,
        "a simple consumer, or a tool resetting offsets"
    );
    // A member that has left cannot commit at its old generation.
    assert_eq!(g.commit(Some("m1"), 1), Some(E::UnknownMemberId));
}

#[test]
fn a_static_members_instance_is_checked_as_well() {
    let mut g = Group::new(100);
    let mut a = join_request(None, &["range"]);
    a.instance = Some(instance("i"));
    let t = g.join(a);
    g.advance(500);
    let told = g.joined(t);
    let member = told.member_id.unwrap();
    let mut req = CommitRequest {
        version: 8,
        member_id: Some(member.clone()),
        instance: Some(instance("i")),
        generation: 1,
        transactional: false,
    };
    let sync = g.sync(member.as_str(), 1, &[]);
    g.synced(sync);
    let ask = |g: &mut Group, req: &CommitRequest| {
        let req = req.clone();
        let t = g.send(|t| Event::Commit(t, req));
        match g.answer(t) {
            Some(Answer::Commit(e)) => e,
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(ask(&mut g, &req), None);
    req.instance = Some(instance("other"));
    assert_eq!(ask(&mut g, &req), Some(E::UnknownMemberId));
}

#[test]
fn a_commit_to_a_group_that_does_not_exist_names_a_generation_only_if_it_is_negative() {
    assert_eq!(commit_to_no_group(-1), None);
    assert_eq!(commit_to_no_group(0), Some(E::IllegalGeneration));
    assert_eq!(commit_to_no_group(5), Some(E::IllegalGeneration));
}
