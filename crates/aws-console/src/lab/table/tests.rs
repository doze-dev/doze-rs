#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::time::Duration;

use aws_gateway::{Ask, Faults, Verdict};

use super::*;
use crate::lab::rule::{Effect, Fail, Matcher, Share};
use crate::lab::testing::{ask, send, slow, table, throttle, throttle_of};

fn distinct(n: usize) -> Spec {
    let (matcher, said) = Matcher::build(&[], &[], &[], &[], &[format!("queue-{n}")]);
    assert!(said.is_empty());
    Spec {
        matcher,
        ..throttle()
    }
}

fn is_pass(v: &Verdict) -> bool {
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
fn a_rule_acts_on_what_it_fits_and_counts_matches_and_hits() {
    let (t, _) = table();
    let id = t.add(throttle_of("sqs")).unwrap();
    assert!(matches!(t.judge(&send()), Verdict::Fail(_)));
    assert!(is_pass(&t.judge(&ask("sns", "SNS", "Publish"))));
    assert!(matches!(t.judge(&send()), Verdict::Fail(_)));
    let r = &t.rules()[0];
    assert_eq!(
        (r.id, r.matched, r.hits),
        (id, 2, 2),
        "an SNS call is not counted"
    );
}

#[test]
fn every_nth_fires_on_the_nth_matching_request() {
    let (t, _) = table();
    t.add(Spec {
        share: Share::Nth(3),
        ..throttle_of("sqs")
    })
    .unwrap();
    let verdicts: Vec<bool> = (0..9).map(|_| !is_pass(&t.judge(&send()))).collect();
    assert_eq!(
        verdicts,
        [false, false, true, false, false, true, false, false, true]
    );
    // A call of another service does not count towards the Nth.
    assert!(is_pass(&t.judge(&ask("sns", "SNS", "Publish"))));
    let r = &t.rules()[0];
    assert_eq!((r.matched, r.hits), (9, 3));
}

#[test]
fn a_percent_share_is_the_seeds_and_close_to_the_share() {
    let run = |seed: u64| {
        let clock = crate::lab::testing::Manual::default();
        let t = LabTable::with(Box::new(clock), seed);
        t.add(Spec {
            share: Share::Percent(30),
            ..throttle()
        })
        .unwrap();
        (0..2000)
            .map(|_| !is_pass(&t.judge(&send())))
            .collect::<Vec<_>>()
    };
    let first = run(7);
    assert_eq!(
        first,
        run(7),
        "the same seed, the same calls, the same hits"
    );
    assert_ne!(first, run(8));
    let hits = first.iter().filter(|h| **h).count();
    assert!(
        (500..700).contains(&hits),
        "30 % of 2000 is about 600: {hits}"
    );
}

#[test]
fn jitter_is_added_within_its_bound_and_is_the_seeds() {
    let run = |seed: u64| {
        let clock = crate::lab::testing::Manual::default();
        let t = LabTable::with(Box::new(clock), seed);
        t.add(Spec {
            effect: Effect::Slow {
                delay_ms: 100,
                jitter_ms: 50,
            },
            ..throttle()
        })
        .unwrap();
        (0..200)
            .map(|_| match t.judge(&send()) {
                Verdict::Slow { delay, .. } => delay,
                other => panic!("{other:?}"),
            })
            .collect::<Vec<_>>()
    };
    let delays = run(3);
    assert_eq!(delays, run(3));
    assert!(delays.iter().all(|d| (100..=150).contains(&d.as_millis())));
    assert!(
        delays.iter().any(|d| d.as_millis() > 100),
        "some jitter is drawn"
    );
    assert!(delays.iter().any(|d| d.as_millis() < 125));
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
fn the_worse_effect_wins_and_a_longer_delay_beats_a_shorter() {
    let (t, _) = table();
    let short = t.add(slow(100)).unwrap();
    let (m, _) = Matcher::build(&["sqs".to_owned()], &[], &[], &[], &[]);
    let long = t
        .add(Spec {
            matcher: m,
            ..slow(400)
        })
        .unwrap();
    match t.judge(&send()) {
        Verdict::Slow { delay, tag } => {
            assert_eq!(delay, Duration::from_millis(400));
            assert_eq!(tag.rule, long);
        }
        other => panic!("{other:?}"),
    }
    // Only the winner counts a hit; both counted the match.
    let by = |id| t.rules().into_iter().find(|r| r.id == id).unwrap();
    assert_eq!((by(long).matched, by(long).hits), (1, 1));
    assert_eq!((by(short).matched, by(short).hits), (1, 0));
    // A fail beats a delay, and a drop beats a fail.
    let (m, _) = Matcher::build(&[], &["SendMessage".to_owned()], &[], &[], &[]);
    let fail = t
        .add(Spec {
            matcher: m,
            ..throttle()
        })
        .unwrap();
    assert!(matches!(t.judge(&send()), Verdict::Fail(f) if f.tag.rule == fail));
    let (m, _) = Matcher::build(&[], &[], &["us-east-1".to_owned()], &[], &[]);
    let drop = t
        .add(Spec {
            matcher: m,
            effect: Effect::Drop,
            ..throttle()
        })
        .unwrap();
    assert!(matches!(t.judge(&send()), Verdict::Drop { tag } if tag.rule == drop));
}

#[test]
fn what_a_rule_remembers_of_callers_is_bounded() {
    let (t, _) = table();
    t.add(throttle()).unwrap();
    for n in 0..12 {
        let key = format!("AKID{n:02}");
        let call = Ask {
            access_key_id: &key,
            ..send()
        };
        assert!(!is_pass(&t.judge(&call)));
    }
    let r = &t.rules()[0];
    assert_eq!(r.callers.len(), MOST_KEPT_CALLERS);
    assert_eq!(r.more_callers, 4, "the hits of the rest are counted");
    assert_eq!(&*r.callers[0], "AKID00");
    assert_eq!(r.hits, 12);
}

#[test]
fn a_rule_remembers_the_first_and_the_last_call_it_fired_on() {
    let (t, clock) = table();
    t.add(throttle_of("sqs")).unwrap();
    assert!(!is_pass(&t.judge(&send())));
    clock.advance(Duration::from_secs(3));
    let other = Ask {
        operation: "ReceiveMessage",
        ..send()
    };
    assert!(!is_pass(&t.judge(&other)));
    let r = &t.rules()[0];
    let first = r.first.as_ref().unwrap();
    assert_eq!(
        (&*first.operation, first.service, &*first.caller),
        ("SendMessage", "SQS", "AKIDLAB")
    );
    assert_eq!(
        first.resource.as_deref(),
        Some("http://localhost:4566/000000000000/orders")
    );
    assert_eq!(first.sdk.as_deref(), Some("boto3 1.34.69"));
    assert_eq!(r.last_ms.unwrap() - first.at_ms, 3000);
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

#[test]
fn a_fail_rule_answers_with_the_error_of_the_service_it_fits() {
    let (t, _) = table();
    t.add(throttle()).unwrap();
    let said = |a: &Ask<'_>| match t.judge(a) {
        Verdict::Fail(f) => (f.shape, f.status, f.code.into_owned()),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        said(&send()),
        (Some("RequestThrottled"), 403, "RequestThrottled".to_owned())
    );
    assert_eq!(
        said(&ask("dynamodb", "DynamoDB", "GetItem")),
        (
            Some("ProvisionedThroughputExceededException"),
            400,
            "ProvisionedThroughputExceededException".to_owned()
        )
    );
    assert_eq!(
        said(&ask("s3", "S3", "GetObject")),
        (None, 503, "SlowDown".to_owned())
    );
    assert_eq!(
        said(&ask("sts", "STS", "GetCallerIdentity")),
        (None, 400, "ThrottlingException".to_owned())
    );
    let (u, _) = table();
    u.add(Spec {
        effect: Effect::Fail(Fail::Custom {
            status: 418,
            code: "Teapot".into(),
        }),
        ..throttle()
    })
    .unwrap();
    match u.judge(&send()) {
        Verdict::Fail(f) => {
            assert_eq!((f.shape, f.status, &*f.code), (None, 418, "Teapot"));
            assert!(f.message.contains("rule 1"), "{}", f.message);
        }
        other => panic!("{other:?}"),
    }
}
