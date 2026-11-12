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
fn the_steady_sentence_says_how_long_and_how_it_stopped_and_what_it_holds() {
    let v = fixtures::home(HomeState::Steady);
    assert_eq!(home_title(&v.state), "Home");
    assert_eq!(
        plain(&home_sub(&v)),
        "Running 3 d 4 h, last stop clean. It holds 9 topics and 21 partitions."
    );
}

#[test]
fn the_steady_sentence_has_a_form_for_each_last_stop() {
    let mut v = fixtures::home(HomeState::Steady);
    v.instance.last_stop = StopView::Unclean;
    assert!(plain(&home_sub(&v)).starts_with("Running 3 d 4 h, last stop not clean. It holds"));
    v.instance.last_stop = StopView::None;
    v.instance.up_secs = 7;
    assert_eq!(
        plain(&home_sub(&v)),
        "Running 7 s on a new data directory. It holds 9 topics and 21 partitions."
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
fn the_instance_lede_follows_the_last_stop() {
    let mut i = fixtures::home(HomeState::Steady).instance;
    assert_eq!(
        instance_lede(&i),
        "Up 3 d 4 h. The last stop was clean, so every partition was trusted on start."
    );
    i.last_stop = StopView::Unclean;
    assert!(instance_lede(&i).contains("not clean, so every partition was checked"));
    i.last_stop = StopView::None;
    i.up_secs = 120;
    assert_eq!(
        instance_lede(&i),
        "Started 2 min ago on a new data directory."
    );
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
fn the_advertised_ledes_say_whether_the_two_addresses_are_one() {
    let mut p = fixtures::probe_ok();
    assert_eq!(
        home_advertised_lede(&p),
        "A client asks for Metadata and then reconnects to the address in the answer, not to the one you gave it. Here that is 127.0.0.1:9092, the address the listener binds."
    );
    assert_eq!(
        plain(&connect_advertised_lede(&p)),
        "After Metadata a client reconnects to the advertised address. This broker advertises 127.0.0.1:9092, which is the address it listens on, and answers there."
    );
    assert_eq!(
        advertised_instance_lede(&p),
        "Advertised host and port equal the listener, and the check reached that address."
    );
    p.same = false;
    p.advertised = "kafka.test:19092".into();
    p.verdict = Verdict::NotReachable;
    assert!(home_advertised_lede(&p).ends_with(
        "Here that is kafka.test:19092, not the address the listener binds (127.0.0.1:9092)."
    ));
    assert!(plain(&connect_advertised_lede(&p)).ends_with("kafka.test:19092, not the address it listens on (127.0.0.1:9092), and does not answer there."));
    assert!(advertised_instance_lede(&p).contains("differ from the listener"));
}

#[test]
fn the_instance_lede_claims_only_what_the_check_found() {
    let mut p = fixtures::probe_ok();
    // Nothing a check made once can promise "always": the words say what was done.
    assert!(!advertised_instance_lede(&p).contains("always"));
    // A listener that did not answer is never described as one a client reached.
    p.same = false;
    p.advertised = "kafka.test:19092".into();
    p.verdict = Verdict::NotReachable;
    p.rules[0].ok = false;
    let said = advertised_instance_lede(&p);
    assert!(!said.contains("reached"), "{said}");
    assert!(
        said.contains("kafka.test:19092") && said.contains(&p.listen),
        "{said}"
    );
    // Not finished: nothing is claimed.
    p.verdict = Verdict::NotChecked;
    assert_eq!(
        advertised_instance_lede(&p),
        "The check did not finish in time: nothing is claimed. Check again."
    );
}

#[test]
fn a_verdict_has_one_word() {
    assert_eq!(verdict_word(Verdict::Reachable), "Reachable");
    assert_eq!(verdict_word(Verdict::NotReachable), "Not reachable");
    assert_eq!(verdict_word(Verdict::NotChecked), "Not checked");
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
