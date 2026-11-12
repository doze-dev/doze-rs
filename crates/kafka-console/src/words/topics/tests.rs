use super::*;
use crate::fixtures_topics as world;
use crate::model::{MessageQuery, Reading};
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
fn a_timeout_is_in_seconds_up_to_two_minutes_as_the_board_writes_it() {
    assert_eq!(timeout(60_000), "60 s");
    assert_eq!(timeout(120_000), "120 s");
    assert_eq!(timeout(120_001), "2 min");
    assert_eq!(timeout(900_000), "15 min");
    assert_eq!(timeout(1_500), "1 s");
    assert_eq!(timeout(0), "0 s");
}

#[test]
fn retention_is_a_span_or_for_ever() {
    assert_eq!(retention(604_800_000), "7 d");
    assert_eq!(retention(3_600_000), "1 h");
    assert_eq!(retention(-1), "for ever");
    assert_eq!(retention(0), "0 s");
}

#[test]
fn the_list_sentence_is_the_boards() {
    let v = world::topics("", false, None);
    assert_eq!(
        plain(&list_sentence(&v)),
        "9 topics hold 21 partitions. The newest write was 0.2 s ago, to clickstream. One internal topic is hidden."
    );
    let shown = world::topics("", true, None);
    assert!(plain(&list_sentence(&shown)).ends_with("The internal topic is shown."));
    assert!(!plain(&list_sentence(&world::topics("pick", false, None))).contains("newest write"));
}

#[test]
fn a_topic_with_an_open_transaction_says_how_far_behind_read_committed_is() {
    let h = world::head("orders").unwrap();
    assert_eq!(
        plain(&topic_sentence(&h)),
        "orders has 3 partitions, keeps records for 7 d, holds 48,213 records (7.9 MiB) and was last written 4.0 s ago. \
         A transaction is open on orders-0 and orders-1: read_committed consumers are held 6 records behind on orders-0."
            .replace("4.0 s ago", "4 s ago")
    );
}

#[test]
fn an_empty_topic_says_so() {
    let h = world::head("orders.dlq").unwrap();
    assert_eq!(
        plain(&topic_sentence(&h)),
        "orders.dlq has 1 partition, keeps records for 7 d, and is empty: offsets 0 to 0, no write yet."
    );
}

#[test]
fn an_offline_partition_is_named_and_the_others_are_said_to_work() {
    let mut h = world::head("notifications.email").unwrap();
    assert_eq!(
        plain(&topic_sentence(&h)),
        "notifications.email has 3 partitions. Partitions 0 and 2 work; partition 1 went offline after a failed flush and refuses writes until the machine restarts."
    );
    h.parts[0].offline = Some("x".to_owned());
    assert!(plain(&topic_sentence(&h)).contains(
        "Partition 2 works; partitions 0 and 1 went offline after a failed flush and refuse writes"
    ));
    h.parts[2].offline = Some("x".to_owned());
    assert!(plain(&topic_sentence(&h)).contains("3 partitions are all offline"));
}

#[test]
fn a_compacted_topic_keeps_the_latest_of_each_key() {
    let mut h = world::head("orders.dlq").unwrap();
    h.cleanup = crate::model::Cleanup::Compact;
    assert!(plain(&topic_sentence(&h)).contains("keeps the latest record of each key"));
    h.cleanup = crate::model::Cleanup::Both;
    h.retention_ms = Some(-1);
    assert!(plain(&topic_sentence(&h)).contains("compacts by key and keeps records for ever"));
}

#[test]
fn a_transaction_with_nothing_held_back_yet_does_not_claim_a_lag() {
    let mut h = world::head("orders").unwrap();
    for p in &mut h.parts {
        p.last_stable = p.end;
    }
    assert!(
        plain(&topic_sentence(&h)).ends_with("read_committed consumers are not held back yet.")
    );
}

#[test]
fn the_partitions_sentence_names_the_partitions_a_transaction_holds_back() {
    let h = world::head("orders").unwrap();
    assert_eq!(
        plain(&partitions_sentence(&h)),
        "3 partitions, 48,213 records, 7.9 MiB. The start is 0 on all of them. \
         The last stable offset is below the high watermark on orders-0 and orders-1: an open transaction holds read_committed consumers back."
    );
    let mut moved = world::head("orders.dlq").unwrap();
    moved.parts[0].start = 5;
    assert!(plain(&partitions_sentence(&moved)).contains("The start has moved on orders.dlq-0"));
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
