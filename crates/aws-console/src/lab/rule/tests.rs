#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::time::Duration;

use super::*;
use crate::lab::testing::{ask, send, throttle};

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

fn matcher(services: &[&str], ops: &[&str], resources: &[&str]) -> Matcher {
    let (m, said) = Matcher::build(&words(services), &words(ops), &[], &[], &words(resources));
    assert!(said.is_empty(), "{said:?}");
    m
}

#[test]
fn a_rule_is_checked_against_every_bound() {
    let ok = throttle();
    assert_eq!(ok.check(), Ok(()));
    let with = |f: &dyn Fn(&mut Spec)| {
        let mut s = throttle();
        f(&mut s);
        s.check()
    };
    assert_eq!(
        with(&|s| s.name = "x".repeat(81)),
        Err(AddError::BadName),
        "a name is at most 80 characters"
    );
    assert_eq!(with(&|s| s.name = "x".repeat(80)), Ok(()));
    assert_eq!(
        with(&|s| s.name = "a\nb".to_owned()),
        Err(AddError::BadName)
    );
    assert_eq!(
        with(&|s| s.ttl = Duration::from_millis(999)),
        Err(AddError::BadDuration)
    );
    assert_eq!(with(&|s| s.ttl = Duration::from_secs(1)), Ok(()));
    assert_eq!(
        with(&|s| s.ttl = Duration::from_secs(24 * 3600 + 1)),
        Err(AddError::BadDuration)
    );
    assert_eq!(with(&|s| s.ttl = Duration::from_secs(24 * 3600)), Ok(()));
    assert_eq!(with(&|s| s.max_hits = Some(0)), Err(AddError::BadHits));
    assert_eq!(with(&|s| s.max_hits = Some(1)), Ok(()));
    for bad in [1, 1001] {
        assert_eq!(
            with(&|s| s.share = Share::Nth(bad)),
            Err(AddError::BadShare),
            "every {bad}th"
        );
    }
    for good in [2, 1000] {
        assert_eq!(with(&|s| s.share = Share::Nth(good)), Ok(()));
    }
    for bad in [0, 100] {
        assert_eq!(
            with(&|s| s.share = Share::Percent(bad)),
            Err(AddError::BadShare),
            "{bad} %"
        );
    }
    for good in [1, 99] {
        assert_eq!(with(&|s| s.share = Share::Percent(good)), Ok(()));
    }
}

#[test]
fn a_delay_and_its_jitter_are_bounded() {
    let slow = |delay_ms, jitter_ms| Spec {
        effect: Effect::Slow {
            delay_ms,
            jitter_ms,
        },
        ..throttle()
    };
    assert_eq!(slow(1, 0).check(), Ok(()));
    assert_eq!(slow(60_000, 60_000).check(), Ok(()));
    assert_eq!(slow(0, 0).check(), Err(AddError::BadDelay));
    assert_eq!(slow(60_001, 0).check(), Err(AddError::BadDelay));
    assert_eq!(slow(10, 60_001).check(), Err(AddError::BadDelay));
}

#[test]
fn a_custom_error_has_an_error_status_and_a_plain_code() {
    let custom = |status, code: &str| Spec {
        effect: Effect::Fail(Fail::Custom {
            status,
            code: code.into(),
            shape: None,
        }),
        ..throttle()
    };
    assert_eq!(custom(400, "ThrottlingException").check(), Ok(()));
    assert_eq!(custom(599, "My.Own.Error").check(), Ok(()));
    assert_eq!(custom(200, "X").check(), Err(AddError::BadStatus(200)));
    assert_eq!(custom(399, "X").check(), Err(AddError::BadStatus(399)));
    assert_eq!(custom(600, "X").check(), Err(AddError::BadStatus(600)));
    assert_eq!(custom(500, "").check(), Err(AddError::BadCode));
    assert_eq!(custom(500, "has space").check(), Err(AddError::BadCode));
    assert_eq!(custom(500, "a-dash").check(), Err(AddError::BadCode));
    assert_eq!(custom(500, &"A".repeat(65)).check(), Err(AddError::BadCode));
    assert_eq!(custom(500, &"A".repeat(64)).check(), Ok(()));
    // Both wrong at once: both are said.
    assert_eq!(custom(100, "no good").problems().len(), 2);
}

#[test]
fn a_matcher_is_bounded_in_every_list() {
    let many = |n: usize| (0..n).map(|i| format!("w{i}")).collect::<Vec<_>>();
    for (field, most) in [
        (Field::Services, 16),
        (Field::Operations, 16),
        (Field::Regions, 8),
        (Field::Callers, 8),
        (Field::Resources, 16),
    ] {
        let build = |n: usize| {
            let list = many(n);
            let none: Vec<String> = Vec::new();
            match field {
                Field::Services => Matcher::build(&list, &none, &none, &none, &none),
                Field::Operations => Matcher::build(&none, &list, &none, &none, &none),
                Field::Regions => Matcher::build(&none, &none, &list, &none, &none),
                Field::Callers => Matcher::build(&none, &none, &none, &list, &none),
                Field::Resources => Matcher::build(&none, &none, &none, &none, &list),
            }
        };
        assert_eq!(build(most).1, Vec::new(), "{field:?} up to {most}");
        assert_eq!(
            build(most + 1).1,
            vec![AddError::TooMany(field)],
            "{field:?} past {most}"
        );
        assert!(AddError::TooMany(field).words().contains(field.word()));
    }
}

#[test]
fn hostile_words_are_said_and_left_out() {
    let (m, said) = Matcher::build(
        &words(&["sqs", "", "bad\u{7}name"]),
        &words(&["SendMessage", &"x".repeat(129)]),
        &[],
        &words(&["AK\u{7}IA"]),
        &words(&["", &"p".repeat(257), "ok*", "tab\there"]),
    );
    assert_eq!(m.services.len(), 1, "{m:?}");
    assert_eq!(m.operations.len(), 1);
    assert!(m.callers.is_empty());
    assert_eq!(m.resources.len(), 1);
    assert_eq!(m.resources[0].as_str(), "ok*");
    assert!(said.contains(&AddError::BadEntry(Field::Services)));
    assert!(said.contains(&AddError::BadEntry(Field::Operations)));
    assert!(said.contains(&AddError::BadEntry(Field::Callers)));
    assert!(said.contains(&AddError::BadEntry(Field::Resources)));
    // Every refusal says which field it is about and what to do.
    for e in &said {
        assert!(!e.field().is_empty() && e.words().len() > 20, "{e:?}");
    }
}

#[test]
fn a_matcher_is_the_same_whatever_order_and_case_its_words_came_in() {
    let a = matcher(&["SQS", "sns"], &["SendMessage", "Publish"], &["b*", "a*"]);
    let b = matcher(
        &["sns", "sqs", "SQS"],
        &["Publish", "SendMessage"],
        &["a*", "b*", "a*"],
    );
    assert_eq!(a, b);
}

#[test]
fn each_field_must_fit_and_a_field_left_empty_fits_anything() {
    assert!(Matcher::default().fits(&send()));
    assert!(matcher(&["sqs"], &[], &[]).fits(&send()));
    assert!(
        matcher(&["SQS"], &[], &[]).fits(&send()),
        "an SDK id, in any case"
    );
    assert!(!matcher(&["sns"], &[], &[]).fits(&send()));
    assert!(matcher(&[], &["SendMessage"], &[]).fits(&send()));
    assert!(
        !matcher(&[], &["sendmessage"], &[]).fits(&send()),
        "an operation is exact"
    );
    assert!(!matcher(&[], &["ReceiveMessage"], &[]).fits(&send()));
    let (m, _) = Matcher::build(&[], &[], &words(&["us-east-1"]), &[], &[]);
    assert!(m.fits(&send()));
    let (m, _) = Matcher::build(&[], &[], &words(&["eu-west-1"]), &[], &[]);
    assert!(!m.fits(&send()));
    let (m, _) = Matcher::build(&[], &[], &[], &words(&["AKIDLAB"]), &[]);
    assert!(m.fits(&send()));
    let (m, _) = Matcher::build(&[], &[], &[], &words(&["AKIDOTHER"]), &[]);
    assert!(!m.fits(&send()));
    // Every list that is set must fit.
    assert!(matcher(&["sqs"], &["SendMessage"], &["orders"]).fits(&send()));
    assert!(!matcher(&["sqs"], &["SendMessage"], &["payments"]).fits(&send()));
}

#[test]
fn two_services_that_sign_alike_are_told_apart_by_their_sdk_id() {
    let streams = ask("dynamodb", "DynamoDB Streams", "GetRecords");
    let table = ask("dynamodb", "DynamoDB", "GetItem");
    assert!(matcher(&["dynamodb"], &[], &[]).fits(&streams));
    assert!(matcher(&["dynamodb"], &[], &[]).fits(&table));
    assert!(matcher(&["DynamoDB Streams"], &[], &[]).fits(&streams));
    assert!(!matcher(&["DynamoDB Streams"], &[], &[]).fits(&table));
}

#[test]
fn a_resource_pattern_fits_the_whole_text_or_its_last_name() {
    let m = matcher(&[], &[], &["orders*"]);
    assert!(m.fits(&send()), "the last name of a queue URL");
    let by_url = matcher(&[], &[], &["http://localhost:4566/*/orders"]);
    assert!(by_url.fits(&send()), "the whole text");
    assert!(!matcher(&[], &[], &["payments*"]).fits(&send()));
    let s3 = aws_gateway::Ask {
        resource: "images/2026/cat.png",
        ..ask("s3", "S3", "GetObject")
    };
    assert!(matcher(&[], &[], &["*.png"]).fits(&s3));
    assert!(matcher(&[], &[], &["images/*"]).fits(&s3));
    assert!(!matcher(&[], &[], &["*.jpg"]).fits(&s3));
}

#[test]
fn a_resource_rule_does_not_fit_a_call_that_names_no_resource() {
    let none = ask("sqs", "SQS", "ListQueues");
    assert!(!matcher(&[], &[], &["*"]).fits(&none));
    assert!(Matcher::default().fits(&none));
    assert!(matcher(&["sqs"], &[], &[]).fits(&none));
}

#[test]
fn two_rules_are_the_same_rule_when_their_calls_and_kind_of_effect_are_the_same() {
    let a = throttle();
    let b = Spec {
        effect: Effect::Fail(Fail::Unavailable),
        ..throttle()
    };
    let c = Spec {
        effect: Effect::Drop,
        ..throttle()
    };
    assert!(a.same_as(&b), "throttle and unavailable are both a Fail");
    assert!(!a.same_as(&c));
    let d = Spec {
        matcher: matcher(&["sqs"], &[], &[]),
        ..throttle()
    };
    assert!(!a.same_as(&d));
}

#[test]
fn every_refusal_says_its_field_and_what_to_do() {
    let all = [
        AddError::Full,
        AddError::Off("The Lab is off.".to_owned()),
        AddError::BadName,
        AddError::BadDuration,
        AddError::BadDelay,
        AddError::BadShare,
        AddError::BadHits,
        AddError::BadStatus(200),
        AddError::BadCode,
        AddError::TooMany(Field::Services),
        AddError::BadEntry(Field::Resources),
        AddError::UnknownService("sqz".to_owned()),
        AddError::UnknownOperation("SendMesage".to_owned()),
        AddError::CustomNeeds(Field::Services),
        AddError::CustomNeeds(Field::Operations),
        AddError::NotListed {
            code: "Teapot".to_owned(),
            operations: "SendMessage".to_owned(),
        },
        AddError::NoCommonError {
            operations: "SendMessage".to_owned(),
            several: false,
        },
        AddError::NoCommonError {
            operations: "SendMessage and ReceiveMessage".to_owned(),
            several: true,
        },
        AddError::WrongStatus {
            code: "Teapot".to_owned(),
            listed: 400,
            given: 418,
        },
    ];
    for e in all {
        assert!(!e.field().is_empty(), "{e:?}");
        assert!(e.words().ends_with('.'), "{e:?}: {}", e.words());
    }
}
