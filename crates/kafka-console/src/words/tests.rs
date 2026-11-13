use super::*;
use crate::fixtures;

fn plain(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|s| match s {
            Span::Text(t) | Span::Mono(t) | Span::Strong(t) => t.as_str(),
        })
        .collect()
}

#[test]
fn the_steady_sentence_is_static_and_holds_no_number() {
    let v = fixtures::home(HomeState::Steady);
    assert_eq!(home_title(&v.state), "Home");
    let s = plain(&home_sub(&v));
    assert_eq!(
        s,
        "What this broker holds, who has spoken to it, and whether anything needs you."
    );
    // What used to be in it (how long it ran, the topics, the apps, the last request) is the strip of metric slots on Home.
    assert!(!s.chars().any(|c| c.is_ascii_digit()), "{s}");
    let mut other = v.clone();
    other.spoke = None;
    other.instance.up_secs = 7;
    assert_eq!(
        plain(&home_sub(&other)),
        s,
        "the sentence does not depend on the data"
    );
}

#[test]
fn the_quiet_sentence_names_the_listener_and_claims_no_client() {
    let v = fixtures::home(HomeState::Quiet);
    assert_eq!(home_title(&v.state), "Your broker is up");
    let s = plain(&home_sub(&v));
    assert!(
        s.starts_with("No topics yet, and that is fine. It is listening on 127.0.0.1:9092"),
        "{s}"
    );
    assert!(!s.contains("connected") && !s.contains("apps"), "{s}");
}

#[test]
fn the_instance_lede_follows_the_last_stop_and_says_no_time() {
    let mut i = fixtures::home(HomeState::Steady).instance;
    assert_eq!(
        instance_lede(&i),
        "The last stop was clean, so every partition was trusted on start."
    );
    i.last_stop = StopView::Unclean;
    assert!(instance_lede(&i).contains("not clean, so every partition was checked"));
    i.last_stop = StopView::None;
    i.up_secs = 120;
    assert_eq!(instance_lede(&i), "Started on a new data directory.");
}

#[test]
fn open_partitions_are_of_the_bound_when_there_is_one() {
    let mut i = fixtures::home(HomeState::Steady).instance;
    assert_eq!(open_partitions(&i), "21 of 64");
    i.max_open_partitions = 0;
    assert_eq!(open_partitions(&i), "21");
    i.open_partitions = 1_500;
    assert_eq!(open_partitions(&i), "1,500");
}

#[test]
fn the_attention_headline_counts() {
    assert_eq!(
        attention_headline(1, 0),
        "1 thing needs you: a transaction is close to its timeout."
    );
    assert_eq!(
        attention_headline(3, 0),
        "3 things need you: 3 transactions are close to their timeout."
    );
}

#[test]
fn a_count_that_did_not_look_at_everything_is_a_floor_and_says_so() {
    assert_eq!(
        attention_headline(1, 4),
        "At least 1 transaction is close to its timeout."
    );
    assert_eq!(
        attention_headline(3, 1),
        "At least 3 transactions are close to their timeout."
    );
    assert_eq!(unchecked_note(0), None);
    assert_eq!(
        unchecked_note(1).as_deref(),
        Some("1 open transaction was not checked.")
    );
    assert_eq!(
        unchecked_note(5).as_deref(),
        Some("5 open transactions were not checked.")
    );
}
