use super::*;

fn head(retention_ms: Option<i64>, cleanup: &str) -> TopicHead {
    TopicHead {
        name: "orders".to_owned(),
        cleanup: cleanup.to_owned(),
        partitions: 3,
        retention_ms,
        records: 48_214,
        transactions: Vec::new(),
        held_back: Vec::new(),
    }
}

fn text(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|s| match s {
            Span::Text(t) | Span::Mono(t) | Span::Strong(t) => t.as_str(),
        })
        .collect()
}

#[test]
fn a_time_of_day_has_milliseconds_in_utc() {
    // 2023-11-14 22:13:20.000 UTC.
    assert_eq!(clock_ms(1_700_000_000_000), "22:13:20.000");
    assert_eq!(clock_ms(1_700_000_000_208), "22:13:20.208");
    assert_eq!(clock_ms(0), "00:00:00.000");
    assert_eq!(clock_ms(999), "00:00:00.999");
    assert_eq!(clock_ms(-1), "23:59:59.999");
}

#[test]
fn what_the_broker_waited_for_is_said_by_acks() {
    assert_eq!(acked(-1), "acknowledged after the flush (acks -1)");
    assert_eq!(acked(1), "acknowledged once written (acks 1)");
    assert!(acked(0).starts_with("not acknowledged"));
    assert_eq!(acked(7), "acknowledged");
}

#[test]
fn partitions_are_named_with_the_topic_and_joined_in_words() {
    assert_eq!(partitions_of("orders", vec![]), "");
    assert_eq!(partitions_of("orders", vec![0]), "orders-0");
    assert_eq!(partitions_of("orders", vec![1, 0]), "orders-0 and orders-1");
    assert_eq!(
        partitions_of("orders", vec![2, 0, 1, 1]),
        "orders-0, orders-1 and orders-2"
    );
}

#[test]
fn the_sentence_under_a_topic_says_what_the_broker_read() {
    assert_eq!(
        text(&head_sentence(&head(Some(604_800_000), "delete"))),
        "orders has 3 partitions, keeps records for 7 d, holds 48,214 records and was last written just now."
    );
    assert_eq!(
        text(&head_sentence(&head(Some(-1), "delete"))),
        "orders has 3 partitions, keeps records for ever, holds 48,214 records and was last written just now."
    );
    assert_eq!(
        text(&head_sentence(&head(None, "compact"))),
        "orders has 3 partitions, holds 48,214 records and was last written just now."
    );
    // A compacted topic's retention is not what keeps its records.
    assert!(!text(&head_sentence(&head(Some(604_800_000), "compact"))).contains("keeps records"));
    let mut h = head(Some(604_800_000), "delete");
    h.transactions = vec![("orders-eos".to_owned(), vec![0, 1])];
    h.held_back = vec![(0, 6)];
    assert_eq!(
        text(&head_sentence(&h)),
        "orders has 3 partitions, keeps records for 7 d, holds 48,214 records and was last written just now. A transaction is open on orders-0 and orders-1: read_committed consumers are held 6 records behind on orders-0."
    );
    h.held_back.clear();
    assert!(text(&head_sentence(&h)).ends_with("A transaction is open on orders-0 and orders-1."));
}

#[test]
fn deleting_removes_what_lies_between_the_start_and_the_target() {
    for (start, high, to, want) in [
        (0, 16_104, 16_000, 16_000),
        (0, 16_104, -1, 16_104),
        (100, 16_104, -1, 16_004),
        (100, 16_104, 50, 0),
        (100, 16_104, 100, 0),
        (0, 10, 20, 10),
        (5, 5, -1, 0),
    ] {
        assert_eq!(removed(start, high, to), want, "{start} {high} {to}");
    }
}

#[test]
fn a_group_below_the_new_start_is_told_it_will_be_out_of_range() {
    let g = vec![
        ("payments-workers".to_owned(), 16_092),
        ("slow".to_owned(), 10),
    ];
    let t = text(&positions(&g, 16_000));
    assert!(
        t.contains("payments-workers is at 16,092: it is not affected."),
        "{t}"
    );
    assert!(
        t.contains("slow is at 10: its next fetch is out of range."),
        "{t}"
    );
    assert_eq!(positions(&[], 5), Vec::<Span>::new());
    let many: Vec<_> = (0..5).map(|i| (format!("g{i}"), 1)).collect();
    assert!(text(&positions(&many, 0)).ends_with("2 more groups have committed here."));
}
