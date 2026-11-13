use std::time::Duration;

use kafka_server::fault::{AddError, Effect, Matcher, Pattern, Share, Spec};
use kafka_wire::messages::ApiKey;

use super::*;

fn spec(effect: Effect, share: Share) -> Spec {
    Spec {
        name: String::new(),
        matcher: Matcher {
            apis: vec![ApiKey::Produce],
            topics: vec![Pattern::new("orders").unwrap()],
            ..Matcher::default()
        },
        effect,
        share,
        ttl: Duration::from_secs(600),
        max_hits: None,
    }
}

#[test]
fn a_sentence_is_the_boards_and_holds_no_number_that_follows_the_broker() {
    let slow = spec(
        Effect::Slow {
            delay_ms: 400,
            jitter_ms: 0,
        },
        Share::EveryNth(4),
    );
    assert_eq!(
        sentence(&slow),
        "+400 ms on 1 in 4 Produce requests to orders."
    );
    assert_eq!(default_name(&slow), "Slow produce to orders");
    let mut fail = spec(Effect::Fail { code: 7 }, Share::EveryNth(5));
    fail.matcher.apis = vec![ApiKey::Fetch];
    fail.matcher.topics = vec![Pattern::new("inventory.levels").unwrap()];
    assert_eq!(
        sentence(&fail),
        "Answers 1 in 5 Fetch requests to inventory.levels with REQUEST_TIMED_OUT."
    );
    assert_eq!(default_name(&fail), "Fail inventory.levels fetches");
    let every = spec(
        Effect::Slow {
            delay_ms: 5,
            jitter_ms: 20,
        },
        Share::Every,
    );
    assert_eq!(
        sentence(&every),
        "+5 ms and up to 20 ms more on every Produce request to orders."
    );
    let pct = spec(
        Effect::Slow {
            delay_ms: 5,
            jitter_ms: 0,
        },
        Share::Percent(10),
    );
    assert_eq!(
        sentence(&pct),
        "+5 ms on a random 10 % of Produce requests to orders."
    );
}

#[test]
fn a_drop_says_whose_connections_and_when() {
    let mut once = spec(Effect::Drop, Share::Every);
    once.matcher = Matcher {
        client: Pattern::new("catalog"),
        up_to_conn: Some(5),
        ..Matcher::default()
    };
    assert_eq!(
        sentence(&once),
        "Closes the connections catalog has open now, at their next request."
    );
    assert_eq!(default_name(&once), "Drop catalog once");
    once.matcher.up_to_conn = None;
    assert_eq!(default_name(&once), "Keep dropping catalog");
}

#[test]
fn every_refusal_of_the_engine_has_a_sentence() {
    for e in [
        AddError::TooMany,
        AddError::BadName,
        AddError::BadDuration,
        AddError::BadDelay,
        AddError::BadShare,
        AddError::BadCode,
        AddError::FailNeedsApi,
        AddError::CannotFail(ApiKey::CreateTopics),
        AddError::BadPartition,
        AddError::TooManyTopics,
        AddError::BadHits,
    ] {
        assert!(refused_words(e).ends_with('.'), "{e:?}");
    }
    assert!(refused_words(AddError::CannotFail(ApiKey::CreateTopics)).contains("CreateTopics"));
}

#[test]
fn the_plain_sentence_says_what_the_form_asks_for() {
    let mut slow = spec(
        Effect::Slow {
            delay_ms: 800,
            jitter_ms: 0,
        },
        Share::Every,
    );
    slow.matcher.client = Pattern::new("payments");
    assert_eq!(
        plain(&slow),
        "Every Produce request to orders from payments will wait 800 ms longer before the broker answers. It ends after 10 min, or when you remove it."
    );
    slow.effect = Effect::Slow {
        delay_ms: 2000,
        jitter_ms: 1000,
    };
    slow.max_hits = Some(20);
    assert!(plain(&slow).contains("2000 ms longer, plus up to 1000 ms at random,"));
    assert!(plain(&slow).ends_with(
        "It ends after 20 hits or 10 min, whichever comes first, or when you remove it."
    ));
    let mut fail = spec(Effect::Fail { code: 6 }, Share::EveryNth(5));
    fail.matcher.group = Pattern::new("billing");
    assert_eq!(
        plain(&fail),
        "1 in 5 Produce requests to orders in group billing will be answered NOT_LEADER_OR_FOLLOWER at once; the broker never sees them. It ends after 10 min, or when you remove it."
    );
    let mut any = spec(Effect::Fail { code: 7 }, Share::Percent(10));
    any.matcher.apis.clear();
    any.matcher.topics.clear();
    any.max_hits = Some(1);
    assert!(
        plain(&any).starts_with("A random 10 % of requests will be answered REQUEST_TIMED_OUT")
    );
    assert!(plain(&any).contains("It ends after its first hit or 10 min"));
}
