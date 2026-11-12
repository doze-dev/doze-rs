//! The Lab's forced steps on a consumer group of the new protocol.

use super::*;
use crate::groups::machine::{Force, Forced};
use crate::groups::note::{Cause, What};

fn forced(r: &mut Rig, f: Force) -> (Forced, Vec<Cause>) {
    let mut out = Vec::new();
    r.group
        .force(Token(900), &f, &r.topics, Millis(r.now), &mut out);
    let mut done = None;
    let mut causes = Vec::new();
    for e in out {
        match e {
            Effect::Reply(_, Answer::Forced(d)) => done = Some(d),
            Effect::Note(n) => {
                if let What::Epoch { why, .. } = n.what {
                    causes.push(why.cause);
                }
            }
            other => r.apply(other),
        }
    }
    (done.expect("a forced step is answered"), causes)
}

/// A group of two members, both settled at the epoch the group reached.
fn two() -> Rig {
    let mut r = rig(4);
    r.beat(join("a"));
    r.advance(INTERVAL);
    let b = r.beat(join("b"));
    r.advance(INTERVAL);
    let _ = (b, r.beat(poll("a", r.member("a").epoch)));
    r
}

#[test]
fn a_forced_rebalance_moves_the_epoch_by_one_and_says_it_was_forced() {
    let mut r = two();
    let before = r.view().epoch;
    let (done, causes) = forced(&mut r, Force::Rebalance);
    assert_eq!(done, Forced::Done);
    assert_eq!(r.view().epoch, before + 1);
    assert_eq!(causes, [Cause::Forced]);
}

#[test]
fn a_forced_expiry_removes_the_member_as_a_missed_session_does() {
    let mut timed = two();
    let mut fast = two();
    // The timer path: b is silent, a beats until b's session is up.
    for _ in 0..8 {
        timed.advance(1_000);
        let e = timed.member("a").epoch;
        timed.beat(poll("a", e));
    }
    let (done, causes) = forced(&mut fast, Force::Expire(id("b")));
    assert_eq!(done, Forced::Done);
    assert_eq!(causes, [Cause::SessionExpired]);
    assert_eq!(fast.view().epoch, timed.view().epoch);
    let names = |r: &Rig| r.view().members.into_iter().map(|m| m.id).collect::<Vec<_>>();
    assert_eq!(names(&fast), names(&timed));
    assert_eq!(names(&fast), [id("a")]);
}

#[test]
fn a_member_that_is_not_there_and_a_group_with_nobody_are_said_so() {
    let mut r = two();
    assert_eq!(forced(&mut r, Force::Expire(id("zzz"))).0, Forced::NoSuchMember);
    let mut empty = rig(2);
    assert_eq!(forced(&mut empty, Force::Rebalance).0, Forced::Empty);
}
