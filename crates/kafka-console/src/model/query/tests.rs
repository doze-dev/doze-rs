use super::*;

fn q(s: &str) -> (MessageQuery, Vec<String>) {
    MessageQuery::parse(&crate::form::parse(s))
}

#[test]
fn no_filter_is_the_default_and_says_nothing() {
    let (query, notes) = q("");
    assert_eq!(query, MessageQuery::default());
    assert!(notes.is_empty() && !query.filtered());
    assert_eq!(query.to_query(), "");
}

#[test]
fn each_filter_is_read_and_written_back() {
    let (query, notes) = q(
        "partition=0&offset=16098&key=ord-88410&isolation=read_committed&limit=10&time=2023-11-14+22%3A13",
    );
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(query.partition, Some(0));
    assert_eq!(query.offset, Some(16_098));
    assert_eq!(query.key.as_deref(), Some("ord-88410"));
    assert_eq!(query.reading, Reading::Committed);
    assert_eq!(query.limit, 10);
    assert_eq!(
        query.time,
        Some((1_699_999_980_000, "2023-11-14 22:13".to_owned()))
    );
    assert!(query.filtered());
    let again = q(query.to_query().trim_start_matches('?')).0;
    assert_eq!(again, query, "{}", query.to_query());
}

#[test]
fn the_one_box_is_an_offset_when_it_is_digits_and_a_key_otherwise() {
    assert_eq!(q("find=16098").0.offset, Some(16_098));
    assert_eq!(q("find=ord-88410").0.key.as_deref(), Some("ord-88410"));
    assert_eq!(q("find=16098&offset=5").0.offset, Some(5));
    assert_eq!(q("find=%20%20").0, MessageQuery::default());
}

#[test]
fn a_value_that_cannot_be_used_is_a_sentence_and_the_rest_still_works() {
    for (bad, field) in [
        ("partition=-1", "partition number"),
        ("partition=abc", "partition number"),
        ("partition=99999999999", "partition number"),
        ("offset=-5", "offset"),
        ("offset=NaN", "offset"),
        ("offset=99999999999999999999999", "offset"),
        ("offset=1.5", "offset"),
        ("time=yesterday", "not a time"),
        ("isolation=snapshot", "isolation level"),
        ("limit=0", "number of records"),
        ("limit=-3", "number of records"),
        ("limit=abc", "number of records"),
    ] {
        let (query, notes) = q(&format!("{bad}&key=k"));
        assert_eq!(notes.len(), 1, "{bad}: {notes:?}");
        assert!(notes[0].contains(field), "{bad}: {notes:?}");
        assert_eq!(query.key.as_deref(), Some("k"), "{bad}");
        assert_eq!(
            (
                query.partition,
                query.offset,
                query.time.clone(),
                query.reading,
                query.limit
            ),
            (None, None, None, Reading::Uncommitted, DEFAULT_LIMIT),
            "{bad}"
        );
    }
}

#[test]
fn a_huge_limit_is_cut_and_a_long_key_is_refused_with_a_sentence() {
    let (query, notes) = q("limit=999999999999");
    assert_eq!(query.limit, MOST_LIMIT);
    assert_eq!(notes.len(), 1);
    let (query, notes) = q(&format!("key={}", "k".repeat(MOST_KEY + 1)));
    assert_eq!((query.key, notes.len()), (None, 1));
    let (query, notes) = q(&format!("key={}", "k".repeat(MOST_KEY)));
    assert!(query.key.is_some() && notes.is_empty());
}

#[test]
fn a_hostile_value_reaches_a_sentence_only_clipped_and_escaped_by_debug() {
    let (_, notes) = q(
        "partition=%3Cscript%3E%E2%80%AE+aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    );
    assert_eq!(notes.len(), 1);
    assert!(notes[0].chars().count() < 160, "{}", notes[0]);
    assert!(!notes[0].contains('\u{202e}'), "{}", notes[0]);
}

#[test]
fn the_isolation_words_are_kafkas() {
    assert_eq!(Reading::Committed.word(), "read_committed");
    assert_eq!(Reading::Uncommitted.word(), "read_uncommitted");
}
