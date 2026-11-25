#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::time::Duration;

use aws_gateway::{Faults, Verdict};

use super::*;
use crate::lab::rule::{Matcher, Share};
use crate::lab::testing::{send, slow, table, throttle, throttle_of};

pub(super) fn distinct(n: usize) -> Spec {
    let (matcher, said) = Matcher::build(&[], &[], &[], &[], &[format!("queue-{n}")]);
    assert!(said.is_empty());
    Spec {
        matcher,
        ..throttle()
    }
}

pub(super) fn is_pass(v: &Verdict) -> bool {
    matches!(v, Verdict::Pass)
}

#[test]
fn rules_are_bounded_at_fifty_and_a_replacement_does_not_count_twice() {
    let (t, _) = table();
    for n in 0..50 {
        t.add(distinct(n)).unwrap();
    }
    assert_eq!(t.rules().len(), 50);
    assert_eq!(
        t.add(distinct(50)),
        Err(AddError::Full),
        "the 51st is refused"
    );
    assert!(!t.has_room_for(&distinct(50)));
    // The same calls and the same kind of effect take the place of the old rule, which is over, as replaced.
    assert!(t.has_room_for(&distinct(7)));
    let id = t.add(distinct(7)).unwrap();
    assert_eq!(t.rules().len(), 50);
    assert!(t.rules().iter().any(|r| r.id == id));
    let replaced = t.ended();
    assert_eq!(replaced.len(), 1);
    assert_eq!(replaced[0].why, EndedWhy::Replaced);
}

#[test]
fn a_rule_that_is_not_allowed_is_never_made() {
    let (t, _) = table();
    let bad = Spec {
        share: Share::Percent(100),
        ..throttle()
    };
    assert_eq!(t.add(bad), Err(AddError::BadShare));
    assert!(t.rules().is_empty());
    assert!(!t.armed());
}

#[test]
fn the_history_is_bounded() {
    let (t, _) = table();
    for n in 0..(MOST_ENDED + 30) {
        let id = t.add(distinct(n)).unwrap();
        assert!(t.remove(id));
    }
    let ended = t.ended();
    assert_eq!(ended.len(), MOST_ENDED);
    // Newest first, and the oldest ones are what was let go.
    assert_eq!(ended[0].rule.id, u32::try_from(MOST_ENDED + 30).unwrap());
    assert_eq!(ended[MOST_ENDED - 1].rule.id, 31);
    assert_eq!(t.clear_ended(), MOST_ENDED);
    assert!(t.ended().is_empty());
}

#[test]
fn a_table_with_no_rule_lets_everything_through_and_takes_no_lock() {
    let (t, _) = table();
    assert!(!t.armed());
    // Were the table to look for its lock, this thread would wait for itself.
    let held = t.lock();
    assert!(is_pass(&t.judge(&send())));
    drop(held);
    // A rule switched off does not arm the table either.
    let id = t.add(throttle()).unwrap();
    assert!(t.armed());
    assert!(t.set_enabled(id, false));
    assert!(!t.armed());
    let held = t.lock();
    assert!(is_pass(&t.judge(&send())));
    drop(held);
}

#[test]
fn a_rule_ends_when_its_time_is_up_and_is_kept_as_ended() {
    let (t, clock) = table();
    let id = t
        .add(Spec {
            ttl: Duration::from_secs(10),
            ..throttle()
        })
        .unwrap();
    clock.advance(Duration::from_secs(9));
    assert!(matches!(t.judge(&send()), Verdict::Fail(_)));
    clock.advance(Duration::from_secs(5));
    // Noticed on the next look, dated when it was due.
    assert!(is_pass(&t.judge(&send())));
    assert!(!t.armed());
    let ended = t.ended();
    assert_eq!((ended[0].rule.id, ended[0].why), (id, EndedWhy::Expired));
    assert_eq!(ended[0].at_ms, ended[0].rule.ends_ms);
    assert_eq!(ended[0].at_ms - ended[0].rule.created_ms, 10_000);
    assert_eq!(ended[0].rule.hits, 1);
    let found = t.find(id).unwrap();
    assert_eq!(found.over.map(|(_, why)| why), Some(EndedWhy::Expired));
}

#[test]
fn a_rule_ends_when_it_has_fired_as_often_as_allowed() {
    let (t, _) = table();
    let id = t
        .add(Spec {
            max_hits: Some(2),
            ..throttle()
        })
        .unwrap();
    assert!(!is_pass(&t.judge(&send())));
    assert!(!is_pass(&t.judge(&send())));
    assert!(is_pass(&t.judge(&send())), "the third is let through");
    assert!(!t.armed());
    assert_eq!(t.ended()[0].why, EndedWhy::Spent);
    assert_eq!(t.ended()[0].rule.id, id);
    assert_eq!(t.ended()[0].rule.hits, 2);
}

#[test]
fn a_rule_that_is_switched_off_does_nothing_and_still_runs_out() {
    let (t, clock) = table();
    let id = t
        .add(Spec {
            ttl: Duration::from_secs(5),
            ..throttle()
        })
        .unwrap();
    assert!(t.set_enabled(id, false));
    assert!(is_pass(&t.judge(&send())));
    assert_eq!(
        t.rules()[0].matched,
        0,
        "a switched off rule counts nothing"
    );
    assert!(t.set_enabled(id, true));
    assert!(!is_pass(&t.judge(&send())));
    assert!(t.set_enabled(id, false));
    clock.advance(Duration::from_secs(6));
    assert!(t.rules().is_empty());
    assert_eq!(t.ended()[0].why, EndedWhy::Expired);
    assert!(
        !t.set_enabled(id, true),
        "an ended rule cannot be switched on"
    );
}

#[test]
fn stop_all_ends_every_rule_at_once() {
    let (t, _) = table();
    for n in 0..5 {
        t.add(distinct(n)).unwrap();
    }
    assert_eq!(t.stop_all(), 5);
    assert!(t.rules().is_empty());
    assert!(!t.armed());
    assert_eq!(t.ended().len(), 5);
    assert!(t.ended().iter().all(|e| e.why == EndedWhy::Cleared));
    assert_eq!(t.stop_all(), 0);
}

#[test]
fn a_rule_gets_a_name_when_it_is_given_none_and_numbers_are_never_reused() {
    let (t, _) = table();
    let a = t.add(throttle_of("sqs")).unwrap();
    let b = t
        .add(Spec {
            name: "mine".to_owned(),
            ..slow(5)
        })
        .unwrap();
    assert_eq!(&*t.find(a).unwrap().rule.name, "Throttle sqs");
    assert_eq!(&*t.find(b).unwrap().rule.name, "mine");
    assert!(t.remove(a));
    let c = t.add(throttle_of("sqs")).unwrap();
    assert!(c > b, "{c} after {b}: a number is not reused");
    assert!(!t.remove(a), "an ended rule cannot be removed again");
}
