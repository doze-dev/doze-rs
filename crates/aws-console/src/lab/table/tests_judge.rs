//! What a call meets in the table: shares, jitter, which rule wins, what a rule remembers, and the error a Fail rule answers with.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::time::Duration;

use aws_gateway::{Ask, Faults, Verdict};

use super::tests::is_pass;
use super::*;
use crate::lab::rule::{Effect, Fail, MOST_KEPT_CALLERS, Matcher, Share};
use crate::lab::testing::{ask, send, slow, table, throttle, throttle_of};

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
