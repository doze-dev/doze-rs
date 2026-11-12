//! A member's rights as the group sees them (SEC-230 to SEC-233): a change of right or of principal takes effect at the
//! very heartbeat, inside the assignment interval, and what is taken away is given up inside the rebalance timeout.

use std::collections::BTreeSet;

use kafka_auth::Principal;

use super::*;
use crate::Caller;
use crate::authorizer::{Authorizer, TopicRights};

/// What a caller gets of a broker that refuses by default and has no ACL: it may DESCRIBE nothing.
fn describes_nothing() -> Option<TopicRights> {
    let broker = Authorizer::new(Vec::new(), false, BTreeSet::new());
    broker.describe_rights(&Caller::new(Principal::user("bob"), None))
}

#[test]
fn a_right_taken_away_withdraws_what_the_member_owns_before_the_assignment_interval_is_over() {
    let mut r = rig(3);
    let joined = r.beat(join("m"));
    assert_eq!(joined.assignment, Some(parts(&[0, 1, 2])));
    // No time has passed, so no target is due: the heartbeat that brings the DENY must still leave the topic out.
    let beat = Heartbeat {
        rights: describes_nothing(),
        ..poll_owning("m", 2, &[0, 1, 2])
    };
    let answer = r.beat(beat);
    assert_eq!(answer.assignment, Some(parts(&[])), "{answer:?}");
}

#[test]
fn what_is_taken_away_is_given_up_inside_the_rebalance_timeout_or_the_member_goes() {
    let mut r = rig(3);
    r.beat(join("m"));
    let beat = || Heartbeat {
        rights: describes_nothing(),
        ..poll_owning("m", 2, &[0, 1, 2])
    };
    r.beat(beat());
    // It never says it has let go: its time to give up (the 60 s it joined with) ends its membership.
    let mut waited = 0;
    loop {
        r.advance(1_000);
        waited += 1_000;
        let answer = r.beat(beat());
        if answer.error.is_some() {
            break;
        }
        assert!(waited <= 61_000, "still a member after {waited} ms");
    }
    assert!(waited >= 59_000, "removed after {waited} ms");
}

#[test]
fn a_static_member_replaced_by_a_principal_that_may_describe_nothing_inherits_nothing() {
    let mut r = rig(3);
    let a = Heartbeat {
        instance: Some(instance("i")),
        ..join("a")
    };
    r.beat(a.clone());
    let leave = Heartbeat {
        epoch: -2,
        ..a.clone()
    };
    r.beat(leave);
    let b = Heartbeat {
        instance: Some(instance("i")),
        rights: describes_nothing(),
        ..join("b")
    };
    let answer = r.beat(b);
    assert_eq!(answer.assignment, Some(parts(&[])), "{answer:?}");
    let seen = r.member("b");
    assert!(seen.subscribed.is_empty(), "{seen:?}");
    assert!(
        seen.target.is_empty() && seen.assigned.is_empty(),
        "{seen:?}"
    );
}
