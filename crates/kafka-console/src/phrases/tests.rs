use super::*;

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
