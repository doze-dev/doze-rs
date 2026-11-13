use super::*;
use crate::fixtures_topics as world;
use crate::model::{MessageQuery, Reading, Span};
use crate::words::reach_sentence;

fn plain(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|s| match s {
            Span::Text(t) | Span::Mono(t) | Span::Strong(t) => t.as_str(),
        })
        .collect()
}

#[test]
fn a_list_is_said_with_and() {
    assert_eq!(list_and::<i32>(&[]), "");
    assert_eq!(list_and(&[5]), "5");
    assert_eq!(list_and(&[0, 2]), "0 and 2");
    assert_eq!(list_and(&[0, 1, 3]), "0, 1 and 3");
}

#[test]
fn retention_is_a_span_or_for_ever() {
    assert_eq!(retention(604_800_000), "7 d");
    assert_eq!(retention(3_600_000), "1 h");
    assert_eq!(retention(-1), "for ever");
    assert_eq!(retention(0), "0 s");
}

#[test]
fn what_a_read_did_is_said_in_the_boards_words() {
    let q = MessageQuery {
        partitions: vec![0],
        offset: Some(16_098),
        ..MessageQuery::default()
    };
    let v = world::messages("orders", q, vec![]).unwrap();
    assert_eq!(
        plain(&reach_sentence(&v)),
        "The fetch returned 6 records, offsets 16,098 to 16,103, the end of the partition. \
         All 6 are at or above the last stable offset (16,098): a read_committed consumer would get none of them."
    );
    let none = world::messages(
        "orders",
        MessageQuery {
            reading: Reading::Committed,
            ..MessageQuery::default()
        },
        vec![],
    )
    .unwrap();
    assert!(!plain(&reach_sentence(&none)).contains("at or above"));
}

#[test]
fn a_search_that_found_nothing_says_how_much_it_looked_at() {
    let mut v = world::messages("orders", MessageQuery::default(), vec![]).unwrap();
    v.rows.clear();
    v.reach.returned = 0;
    v.reach.scanned = Some((1_200, true));
    assert_eq!(
        plain(&reach_sentence(&v)),
        "No record matched. The search looked at all 1,200 records of the partition."
    );
    v.reach.scanned = Some((8_000, false));
    assert!(
        plain(&reach_sentence(&v))
            .contains("8,000 newest records the search looked at; it stopped at its limit")
    );
}
