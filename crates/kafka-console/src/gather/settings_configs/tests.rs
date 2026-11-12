use super::*;

fn text(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|s| match s {
            Span::Text(t) | Span::Mono(t) | Span::Strong(t) => t.as_str(),
        })
        .collect()
}

fn names(n: &[&str]) -> Vec<String> {
    n.iter().map(|s| (*s).to_owned()).collect()
}

fn board_world() -> Followers {
    Followers {
        following: names(&[
            "payments.settled",
            "pick-lists",
            "pick-lists.dlq",
            "orders.dlq",
        ]),
        own: vec![
            ("orders".into(), "604800000".into()),
            ("notifications.email".into(), "86400000".into()),
            ("clickstream".into(), "3600000".into()),
        ],
        compact_only: names(&["inventory.levels", "customer.profiles"]),
        asked: 9,
        total: 9,
    }
}

#[test]
fn who_sees_it_says_the_topics_that_follow_the_ones_that_keep_theirs_and_the_ones_retention_skips()
{
    let (main, dim) = who_sees("log.retention.ms", &board_world());
    assert_eq!(
        text(&main),
        "the topics with no retention.ms of their own: payments.settled, pick-lists, pick-lists.dlq, orders.dlq"
    );
    assert_eq!(
        dim,
        "orders (7 d), notifications.email (1 d), clickstream (1 h) keep theirs. inventory.levels, customer.profiles are compact only: retention does not apply."
    );
}

#[test]
fn a_long_list_is_cut_and_a_cut_look_says_so() {
    let many: Vec<String> = (0..9).map(|i| format!("t{i}")).collect();
    let f = Followers {
        following: many,
        asked: 200,
        total: 250,
        ..Followers::default()
    };
    let (main, dim) = who_sees("log.segment.bytes", &f);
    assert!(
        text(&main).ends_with("t0, t1, t2, t3, t4, t5 and 3 more"),
        "{}",
        text(&main)
    );
    assert!(
        dim.contains("Only the first 200 of 250 topics were looked at."),
        "{dim}"
    );
}

#[test]
fn no_topics_and_no_followers_have_their_own_sentences() {
    let (main, _) = who_sees("log.retention.ms", &Followers::default());
    assert!(text(&main).starts_with("no topic yet"), "{}", text(&main));
    let all_own = Followers {
        own: vec![("a".into(), "1".into())],
        asked: 1,
        total: 1,
        ..Followers::default()
    };
    let (main, _) = who_sees("log.retention.ms", &all_own);
    assert!(
        text(&main).contains("every topic sets its own retention.ms"),
        "{}",
        text(&main)
    );
    assert!(
        text(&who_now_sees("log.retention.ms", "5", &all_own)).contains("nothing changes for them")
    );
}

#[test]
fn after_a_save_the_card_says_how_many_topics_now_follow_the_value() {
    let said = text(&who_now_sees(
        "log.retention.ms",
        "259200000",
        &board_world(),
    ));
    assert_eq!(
        said,
        "4 topics have no retention.ms of their own and now follow 3 d: payments.settled, pick-lists, pick-lists.dlq, orders.dlq. The topics that set their own are unchanged."
    );
    let one = Followers {
        following: names(&["a"]),
        asked: 1,
        total: 1,
        ..Followers::default()
    };
    assert!(
        text(&who_now_sees("log.segment.bytes", "2097152", &one))
            .starts_with("1 topic has no segment.bytes of their own")
    );
}

#[test]
fn a_row_is_set_at_run_time_only_for_a_dynamic_source() {
    let mk = |source| SettingsConfigRow {
        name: "x".into(),
        what: String::new(),
        value: String::new(),
        gloss: None,
        source,
        source_note: String::new(),
        takes_effect: String::new(),
    };
    let rows = vec![
        mk(ConfigSource::DynamicBroker),
        mk(ConfigSource::DynamicDefault),
        mk(ConfigSource::StaticFlag),
        mk(ConfigSource::Default),
    ];
    assert_eq!(set_count(&rows), 2);
    assert_eq!(source_label(ConfigSource::StaticFlag), "Static broker");
    assert_eq!(
        source_label(ConfigSource::DynamicDefault),
        "Dynamic default broker"
    );
}
