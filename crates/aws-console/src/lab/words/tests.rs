#![allow(clippy::unwrap_used)]

use std::time::Duration;

use super::*;
use crate::lab::rule::{Effect, Fail, Matcher, Share, Spec};

fn spec(effect: Effect, share: Share) -> Spec {
    Spec {
        name: String::new(),
        matcher: Matcher::default(),
        effect,
        share,
        ttl: Duration::from_secs(600),
        max_hits: None,
    }
}

fn with(m: (&[&str], &[&str], &[&str]), s: Spec) -> Spec {
    let w = |l: &[&str]| l.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
    let (matcher, said) = Matcher::build(&w(m.0), &w(m.1), &[], &[], &w(m.2));
    assert!(said.is_empty());
    Spec { matcher, ..s }
}

fn sentence_of(s: &Spec) -> String {
    sentence(s, &|w| match w {
        "sqs" => "SQS".to_owned(),
        "states" => "Step Functions".to_owned(),
        other => other.to_owned(),
    })
}

#[test]
fn a_rule_is_a_sentence_made_once() {
    let s = with(
        (&["sqs"], &["SendMessage"], &["orders"]),
        spec(Effect::Fail(Fail::Throttle), Share::Nth(4)),
    );
    assert_eq!(
        sentence_of(&s),
        "Throttle 1 in 4 SQS SendMessage calls to orders for 10 minutes"
    );
    assert_eq!(
        sentence_of(&spec(Effect::Drop, Share::Every)),
        "Close the connection of every call for 10 minutes"
    );
    let slow = Spec {
        max_hits: Some(1),
        ttl: Duration::from_secs(90),
        ..with(
            (&["states"], &[], &[]),
            spec(
                Effect::Slow {
                    delay_ms: 400,
                    jitter_ms: 100,
                },
                Share::Percent(30),
            ),
        )
    };
    assert_eq!(
        sentence_of(&slow),
        "Delay 30 % of Step Functions calls by 400 ms and up to 100 ms more for 90 seconds, once"
    );
    let custom = Spec {
        max_hits: Some(20),
        ..spec(
            Effect::Fail(Fail::Custom {
                status: 418,
                code: "Teapot".into(),
            }),
            Share::Every,
        )
    };
    assert_eq!(
        sentence_of(&custom),
        "Answer Teapot (418) to every call for 10 minutes, at most 20 times"
    );
}

#[test]
fn every_kind_of_failure_has_its_own_verb() {
    let verbs: Vec<String> = [
        Fail::Throttle,
        Fail::Unavailable,
        Fail::Internal,
        Fail::AccessDenied,
    ]
    .into_iter()
    .map(|f| sentence_of(&spec(Effect::Fail(f), Share::Every)))
    .collect();
    assert_eq!(verbs.len(), 4);
    for (i, a) in verbs.iter().enumerate() {
        for b in &verbs[i + 1..] {
            assert_ne!(a, b);
        }
    }
    assert!(verbs[1].contains("503") && verbs[2].contains("500"));
}

#[test]
fn lists_are_said_in_words_and_counted_past_three() {
    let s = with(
        (
            &["sqs", "sns", "s3", "kms"],
            &["A", "B", "C", "D", "E"],
            &[],
        ),
        spec(Effect::Drop, Share::Every),
    );
    let said = sentence(&s, &ToString::to_string);
    assert!(said.contains("4 services 5 operations call"), "{said}");
    let two = with(
        (&["sqs", "sns"], &[], &["a*", "b*"]),
        spec(Effect::Drop, Share::Every),
    );
    assert_eq!(
        sentence(&two, &ToString::to_string),
        "Close the connection of every sns and sqs call to a* and b* for 10 minutes"
    );
    assert_eq!(target(&two), "a* and b*");
    assert_eq!(target(&spec(Effect::Drop, Share::Every)), "every resource");
}

#[test]
fn how_often_and_how_long_are_words() {
    assert_eq!(share_words(Share::Every), "every call");
    assert_eq!(share_words(Share::Nth(4)), "1 in 4");
    assert_eq!(share_words(Share::Percent(30)), "30 %");
    for (secs, words) in [
        (1, "1 second"),
        (59, "59 seconds"),
        (60, "1 minute"),
        (90, "90 seconds"),
        (600, "10 minutes"),
        (3600, "1 hour"),
        (7200, "2 hours"),
        (3660, "61 minutes"),
        (86_400, "24 hours"),
    ] {
        assert_eq!(duration_words(Duration::from_secs(secs)), words);
    }
}

#[test]
fn a_rule_without_a_name_is_named_by_what_it_does_and_never_too_long() {
    assert_eq!(
        default_name(&with(
            (&["sqs"], &["SendMessage"], &[]),
            spec(Effect::Fail(Fail::Throttle), Share::Every)
        )),
        "Throttle sqs SendMessage"
    );
    assert_eq!(
        default_name(&spec(Effect::Drop, Share::Every)),
        "Drop every call"
    );
    let ops: Vec<String> = (0..3).map(|i| format!("{}{i}", "O".repeat(40))).collect();
    let refs: Vec<&str> = ops.iter().map(String::as_str).collect();
    let long = default_name(&with((&[], &refs, &[]), spec(Effect::Drop, Share::Every)));
    assert_eq!(long.chars().count(), 80);
}
