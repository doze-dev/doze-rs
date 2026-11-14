use std::time::Duration;

use kafka_auth::Principal;
use kafka_wire::api::Request;
use kafka_wire::header::RequestHead;
use kafka_wire::messages::ApiKey;

use super::*;
use crate::fault::rule::{Matcher, Pattern};
use crate::fault::testing::{Manual, ask, fetch, head, produce};

fn table(clock: &Manual) -> Arc<FaultTable> {
    FaultTable::with(Box::new(clock.clone()), 42)
}

fn topic_rule(topic: &str, api: ApiKey, effect: Effect) -> Spec {
    Spec {
        name: String::new(),
        matcher: Matcher {
            apis: vec![api],
            topics: vec![Pattern::new(topic).unwrap()],
            ..Matcher::default()
        },
        effect,
        share: Share::Every,
        ttl: Duration::from_secs(600),
        max_hits: None,
    }
}

const SLOW: Effect = Effect::Slow {
    delay_ms: 400,
    jitter_ms: 0,
};

/// Asks the table about a Produce to `topic` from `client`.
fn judge(t: &FaultTable, client: &str, topic: &str) -> Verdict {
    judge_conn(t, Some(1), client, topic)
}

fn judge_conn(t: &FaultTable, conn: Option<u64>, client: &str, topic: &str) -> Verdict {
    let (h, r): (RequestHead, Request) =
        (head(ApiKey::Produce, 9, client), produce(topic, &[0], 1));
    let who = Principal::anonymous();
    t.judge(&ask(conn, &who, &h, Some(&r)))
}

fn applied(v: &Verdict) -> Option<Applied> {
    match v {
        Verdict::Pass => None,
        Verdict::Act(a) => Some(a.effect),
    }
}

#[test]
fn a_table_with_no_rule_lets_everything_through_and_takes_no_lock() {
    let t = table(&Manual::default());
    assert!(matches!(judge(&t, "a", "orders"), Verdict::Pass));
    assert!(t.is_empty());
}

#[test]
fn a_rule_acts_on_what_it_fits_and_counts_matches_and_hits() {
    let clock = Manual::default();
    let t = table(&clock);
    let id = t.add(topic_rule("orders", ApiKey::Produce, SLOW)).unwrap();
    assert_eq!(
        applied(&judge(&t, "orders-go", "orders")),
        Some(Applied::Delay(Duration::from_millis(400)))
    );
    assert!(matches!(judge(&t, "orders-go", "payments"), Verdict::Pass));
    let (h, r) = (head(ApiKey::Fetch, 12, "x"), fetch("orders", &[0]));
    let who = Principal::anonymous();
    assert!(matches!(
        t.judge(&ask(Some(1), &who, &h, Some(&r))),
        Verdict::Pass
    ));
    let rule = t.rule(id).unwrap();
    assert_eq!((rule.matched, rule.hits), (1, 1));
    assert_eq!(rule.clients, vec!["orders-go".into()]);
    let first = rule.first.unwrap();
    assert_eq!(first.api, ApiKey::Produce);
    assert_eq!(first.target, Some(("orders".into(), Some(0))));
    assert_eq!(rule.last.unwrap().correlation, 7);
}

#[test]
fn the_note_names_the_rule_that_acted() {
    let t = table(&Manual::default());
    let mut spec = topic_rule("orders", ApiKey::Produce, SLOW);
    spec.name = "Slow produce to orders".into();
    let id = t.add(spec).unwrap();
    let Verdict::Act(act) = judge(&t, "a", "orders") else {
        panic!("no act")
    };
    assert_eq!(
        (act.note.rule, &*act.note.name),
        (id, "Slow produce to orders")
    );
}

#[test]
fn every_nth_fires_on_the_nth_matching_request() {
    let t = table(&Manual::default());
    let mut spec = topic_rule("orders", ApiKey::Produce, Effect::Fail { code: 7 });
    spec.share = Share::EveryNth(4);
    let id = t.add(spec).unwrap();
    let fired: Vec<bool> = (0..12)
        .map(|_| applied(&judge(&t, "a", "orders")).is_some())
        .collect();
    let at: Vec<usize> = fired
        .iter()
        .enumerate()
        .filter(|(_, f)| **f)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(at, vec![3, 7, 11]);
    let rule = t.rule(id).unwrap();
    assert_eq!((rule.matched, rule.hits), (12, 3));
}

#[test]
fn a_percent_share_is_the_seeds_and_close_to_the_share() {
    let run = |seed: u64| {
        let t = FaultTable::with(Box::new(Manual::default()), seed);
        let mut spec = topic_rule("orders", ApiKey::Produce, SLOW);
        spec.share = Share::Percent(30);
        t.add(spec).unwrap();
        (0..2000)
            .map(|_| applied(&judge(&t, "a", "orders")).is_some())
            .collect::<Vec<_>>()
    };
    let a = run(1);
    assert_eq!(a, run(1), "the same seed draws the same requests");
    assert_ne!(a, run(2));
    let hits = a.iter().filter(|x| **x).count();
    assert!((500..700).contains(&hits), "{hits} of 2000 at 30 %");
}

#[test]
fn jitter_is_added_within_its_bound_and_is_the_seeds() {
    let t = table(&Manual::default());
    t.add(topic_rule(
        "orders",
        ApiKey::Produce,
        Effect::Slow {
            delay_ms: 100,
            jitter_ms: 50,
        },
    ))
    .unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..200 {
        let Some(Applied::Delay(d)) = applied(&judge(&t, "a", "orders")) else {
            panic!("no delay")
        };
        assert!((100..=150).contains(&d.as_millis()), "{d:?}");
        seen.insert(d.as_millis());
    }
    assert!(seen.len() > 10, "{seen:?}");
}

#[test]
fn a_rule_ends_when_its_time_is_up_and_is_kept_as_ended() {
    let clock = Manual::default();
    let t = table(&clock);
    let mut spec = topic_rule("orders", ApiKey::Produce, SLOW);
    spec.ttl = Duration::from_secs(10);
    let id = t.add(spec).unwrap();
    clock.advance(9_999);
    assert!(applied(&judge(&t, "a", "orders")).is_some());
    clock.advance(1);
    assert!(applied(&judge(&t, "a", "orders")).is_none());
    assert!(t.rules().is_empty());
    let ended = t.ended();
    assert_eq!(ended.len(), 1);
    let e = ended.first().unwrap();
    assert_eq!((e.rule.id, e.why, e.rule.hits), (id, EndedWhy::Expired, 1));
    assert_eq!(
        e.at_ms, e.rule.ends_ms,
        "it ended when its time was up, not when it was noticed"
    );
}

#[test]
fn a_rule_ends_when_it_has_fired_as_often_as_allowed() {
    let t = table(&Manual::default());
    let mut spec = topic_rule("orders", ApiKey::Produce, Effect::Fail { code: 7 });
    spec.max_hits = Some(2);
    t.add(spec).unwrap();
    let fired: Vec<bool> = (0..4)
        .map(|_| applied(&judge(&t, "a", "orders")).is_some())
        .collect();
    assert_eq!(fired, vec![true, true, false, false]);
    assert_eq!(t.ended().first().unwrap().why, EndedWhy::Spent);
}

#[test]
fn rules_are_bounded_at_fifty_and_a_replacement_does_not_count_twice() {
    let t = table(&Manual::default());
    for n in 0..MOST_RULES {
        t.add(topic_rule(&format!("t{n}"), ApiKey::Produce, SLOW))
            .unwrap();
    }
    assert_eq!(t.len(), MOST_RULES);
    assert_eq!(
        t.add(topic_rule("one-more", ApiKey::Produce, SLOW)),
        Err(AddError::TooMany)
    );
    // The same matcher and kind of effect takes the old rule's place.
    let mut again = topic_rule(
        "t3",
        ApiKey::Produce,
        Effect::Slow {
            delay_ms: 900,
            jitter_ms: 0,
        },
    );
    again.name = "newer".into();
    t.add(again).unwrap();
    assert_eq!(t.len(), MOST_RULES);
    assert_eq!(t.ended().first().unwrap().why, EndedWhy::Replaced);
}

#[test]
fn the_history_is_bounded() {
    let t = table(&Manual::default());
    for n in 0..(MOST_ENDED + 20) {
        let id = t
            .add(topic_rule(&format!("t{n}"), ApiKey::Produce, SLOW))
            .unwrap();
        assert!(t.remove(id));
    }
    assert_eq!(t.ended().len(), MOST_ENDED);
    assert_eq!(t.ended().first().unwrap().why, EndedWhy::Cleared);
    t.clear_ended();
    assert!(t.ended().is_empty());
}

#[test]
fn a_rule_that_is_switched_off_does_nothing_and_still_runs_out() {
    let clock = Manual::default();
    let t = table(&clock);
    let mut spec = topic_rule("orders", ApiKey::Produce, SLOW);
    spec.ttl = Duration::from_secs(5);
    let id = t.add(spec).unwrap();
    assert!(t.set_enabled(id, false));
    assert!(applied(&judge(&t, "a", "orders")).is_none());
    assert_eq!(t.rule(id).unwrap().matched, 0);
    assert!(t.set_enabled(id, true));
    assert!(applied(&judge(&t, "a", "orders")).is_some());
    assert!(t.set_enabled(id, false));
    clock.advance(5_000);
    assert!(t.rules().is_empty());
    assert!(
        !t.set_enabled(id, true),
        "an ended rule cannot be switched on"
    );
    assert!(!t.remove(id));
}

#[test]
fn the_worse_effect_wins_and_a_longer_delay_beats_a_shorter() {
    let t = table(&Manual::default());
    t.add(topic_rule("orders", ApiKey::Produce, SLOW)).unwrap();
    t.add(topic_rule(
        "ord*",
        ApiKey::Produce,
        Effect::Slow {
            delay_ms: 900,
            jitter_ms: 0,
        },
    ))
    .unwrap();
    assert_eq!(
        applied(&judge(&t, "a", "orders")),
        Some(Applied::Delay(Duration::from_millis(900)))
    );
    t.add(topic_rule("o*", ApiKey::Produce, Effect::Fail { code: 7 }))
        .unwrap();
    assert_eq!(applied(&judge(&t, "a", "orders")), Some(Applied::Fail(7)));
    t.add(topic_rule("*", ApiKey::Produce, Effect::Drop))
        .unwrap();
    assert_eq!(applied(&judge(&t, "a", "orders")), Some(Applied::Drop));
}

#[test]
fn a_drop_once_closes_only_the_connections_that_were_open_when_it_was_made() {
    let t = table(&Manual::default());
    let mut spec = Spec {
        name: String::new(),
        matcher: Matcher {
            client: Some(Pattern::new("catalog").unwrap()),
            up_to_conn: Some(4),
            ..Matcher::default()
        },
        effect: Effect::Drop,
        share: Share::Every,
        ttl: Duration::from_secs(60),
        max_hits: Some(2),
    };
    spec.name = "Drop catalog once".into();
    t.add(spec).unwrap();
    assert!(
        applied(&judge_conn(&t, Some(5), "catalog", "x")).is_none(),
        "a newer connection"
    );
    assert!(
        applied(&judge_conn(&t, Some(2), "other", "x")).is_none(),
        "another app"
    );
    assert_eq!(
        applied(&judge_conn(&t, Some(2), "catalog", "x")),
        Some(Applied::Drop)
    );
    assert_eq!(
        applied(&judge_conn(&t, Some(4), "catalog", "x")),
        Some(Applied::Drop)
    );
    assert!(
        applied(&judge_conn(&t, Some(3), "catalog", "x")).is_none(),
        "it has closed as many as it had to"
    );
}

#[test]
fn the_console_is_never_bent_by_a_rule_that_does_not_name_it() {
    let t = table(&Manual::default());
    t.add(Spec {
        name: String::new(),
        matcher: Matcher::default(),
        effect: Effect::Drop,
        share: Share::Every,
        ttl: Duration::from_secs(60),
        max_hits: None,
    })
    .unwrap();
    let (h, r) = (
        head(ApiKey::Produce, 9, "console"),
        produce("orders", &[0], 1),
    );
    let console = Principal::console();
    assert!(matches!(
        t.judge(&ask(Some(1), &console, &h, Some(&r))),
        Verdict::Pass
    ));
    let other = Principal::user("alice");
    assert!(matches!(
        t.judge(&ask(Some(1), &other, &h, Some(&r))),
        Verdict::Act(_)
    ));
}

#[test]
fn what_a_rule_remembers_of_clients_is_bounded() {
    let t = table(&Manual::default());
    let id = t.add(topic_rule("orders", ApiKey::Produce, SLOW)).unwrap();
    for n in 0..(MOST_CLIENTS + 5) {
        judge(&t, &format!("client-{n}"), "orders");
        judge(&t, "client-0", "orders");
    }
    let rule = t.rule(id).unwrap();
    assert_eq!(rule.clients.len(), MOST_CLIENTS);
    assert_eq!(rule.more_clients, 5);
    let long = "x".repeat(500);
    judge(&t, &long, "orders");
    assert!(t.rule(id).unwrap().first.unwrap().client.len() <= 64);
}

#[test]
fn hostile_names_and_patterns_are_kept_as_text() {
    let t = table(&Manual::default());
    let mut spec = topic_rule("<script>alert(1)</script>*", ApiKey::Produce, SLOW);
    spec.name = "<img src=x onerror=alert(1)>".into();
    let id = t.add(spec).unwrap();
    assert_eq!(&*t.rule(id).unwrap().name, "<img src=x onerror=alert(1)>");
    assert!(applied(&judge(&t, "a", "<script>alert(1)</script>x")).is_some());
}
