use std::time::Duration;

use kafka_wire::messages::ApiKey;

use super::*;

fn pat(s: &str) -> Pattern {
    Pattern::new(s).unwrap()
}

#[test]
fn a_pattern_fits_by_its_stars_and_question_marks() {
    let table = [
        ("orders", "orders", true),
        ("orders", "orders2", false),
        ("orders*", "orders", true),
        ("orders*", "orders.eu", true),
        ("*", "", true),
        ("*.eu", "orders.eu", true),
        ("*.eu", "orders.us", false),
        ("o?ders", "orders", true),
        ("o?ders", "oders", false),
        ("a*b*c", "aXXbYYc", true),
        ("a*b*c", "aXXbYY", false),
        ("a*b*c", "abcabc", true),
        ("**", "x", true),
        ("a**", "a", true),
        ("?", "", false),
        ("a*a", "a", false),
    ];
    for (p, n, want) in table {
        assert_eq!(pat(p).fits(n), want, "{p:?} against {n:?}");
    }
}

#[test]
fn a_hostile_pattern_is_linear_not_exponential() {
    let p = pat(&format!("{}b", "a*".repeat(100)));
    let name = "a".repeat(5000);
    let began = std::time::Instant::now();
    assert!(!p.fits(&name));
    assert!(began.elapsed() < Duration::from_secs(1));
}

#[test]
fn a_pattern_is_not_empty_too_long_or_made_of_control_characters() {
    assert!(Pattern::new("").is_none());
    assert!(Pattern::new(&"x".repeat(250)).is_none());
    assert!(Pattern::new("a\nb").is_none());
    assert!(Pattern::new(&"x".repeat(249)).is_some());
}

fn slow() -> Spec {
    Spec {
        name: String::new(),
        matcher: Matcher::default(),
        effect: Effect::Slow {
            delay_ms: 400,
            jitter_ms: 0,
        },
        share: Share::Every,
        ttl: Duration::from_secs(60),
        max_hits: None,
    }
}

#[test]
fn a_rule_is_checked_against_every_bound() {
    let mut cases: Vec<(Spec, Result<(), AddError>)> = vec![(slow(), Ok(()))];
    let mut s = slow();
    s.ttl = Duration::from_millis(999);
    cases.push((s, Err(AddError::BadDuration)));
    let mut s = slow();
    s.ttl = LONGEST_RULE + Duration::from_secs(1);
    cases.push((s, Err(AddError::BadDuration)));
    let mut s = slow();
    s.ttl = LONGEST_RULE;
    cases.push((s, Ok(())));
    let mut s = slow();
    s.effect = Effect::Slow {
        delay_ms: 0,
        jitter_ms: 0,
    };
    cases.push((s, Err(AddError::BadDelay)));
    let mut s = slow();
    s.effect = Effect::Slow {
        delay_ms: LONGEST_DELAY_MS + 1,
        jitter_ms: 0,
    };
    cases.push((s, Err(AddError::BadDelay)));
    let mut s = slow();
    s.share = Share::EveryNth(1);
    cases.push((s, Err(AddError::BadShare)));
    let mut s = slow();
    s.share = Share::EveryNth(1001);
    cases.push((s, Err(AddError::BadShare)));
    let mut s = slow();
    s.share = Share::Percent(100);
    cases.push((s, Err(AddError::BadShare)));
    let mut s = slow();
    s.share = Share::Percent(0);
    cases.push((s, Err(AddError::BadShare)));
    let mut s = slow();
    s.max_hits = Some(0);
    cases.push((s, Err(AddError::BadHits)));
    let mut s = slow();
    s.name = "a\u{7}b".into();
    cases.push((s, Err(AddError::BadName)));
    let mut s = slow();
    s.name = "x".repeat(LONGEST_NAME + 1);
    cases.push((s, Err(AddError::BadName)));
    let mut s = slow();
    s.matcher.partitions = vec![-1];
    cases.push((s, Err(AddError::BadPartition)));
    for (spec, want) in cases {
        assert_eq!(spec.check(), want, "{spec:?}");
    }
}

#[test]
fn a_rule_that_fails_names_apis_it_can_answer_and_a_code_kafka_has() {
    let fail = |apis: Vec<ApiKey>, code: i16| {
        let mut s = slow();
        s.effect = Effect::Fail { code };
        s.matcher.apis = apis;
        s.check()
    };
    assert_eq!(fail(vec![ApiKey::Produce], 7), Ok(()));
    assert_eq!(fail(vec![], 7), Err(AddError::FailNeedsApi));
    assert_eq!(fail(vec![ApiKey::Produce], 0), Err(AddError::BadCode));
    assert_eq!(fail(vec![ApiKey::Produce], 9999), Err(AddError::BadCode));
    assert_eq!(
        fail(vec![ApiKey::Produce, ApiKey::CreateTopics], 7),
        Err(AddError::CannotFail(ApiKey::CreateTopics))
    );
    assert_eq!(
        fail(vec![ApiKey::SaslAuthenticate], 7),
        Err(AddError::CannotFail(ApiKey::SaslAuthenticate))
    );
}

#[test]
fn a_rule_is_named_after_its_effect_when_it_has_no_name() {
    assert_eq!(slow().display_name(), "slow");
    let mut s = slow();
    s.name = "Slow produce to orders".into();
    assert_eq!(s.display_name(), "Slow produce to orders");
}
