use super::*;
use crate::fixtures_log::{producers, txn_view};

fn text(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|s| match s {
            Span::Text(t) | Span::Mono(t) | Span::Strong(t) => t.as_str(),
        })
        .collect()
}

#[test]
fn the_producers_page_says_what_the_open_transaction_holds_back() {
    let v = producers("orders", 0, Vec::new()).unwrap();
    assert_eq!(
        text(&producers_sentence(&v)),
        "3 producers are remembered on orders-0. One transaction, orders-eos, is open and holds the last stable offset at 16,098 while the high watermark is 16,104: a read_committed consumer is held 6 records behind."
    );
}

#[test]
fn with_no_transaction_the_stable_offset_is_the_high_watermark_and_with_no_producer_it_says_so() {
    let mut v = producers("orders", 0, Vec::new()).unwrap();
    v.open.clear();
    v.last_stable = v.end;
    v.producers.truncate(1);
    assert_eq!(
        text(&producers_sentence(&v)),
        "1 producer is remembered on orders-0. No transaction is open: the last stable offset equals the high watermark, 16,104."
    );
    v.producers.clear();
    assert!(text(&producers_sentence(&v)).starts_with("No producer is remembered on orders-0."));
}

#[test]
fn a_transaction_open_on_the_topic_that_holds_nothing_back_here_says_so() {
    let mut v = producers("orders", 0, Vec::new()).unwrap();
    v.last_stable = v.end;
    assert!(text(&producers_sentence(&v)).ends_with("holds nothing back on orders-0."));
}

#[test]
fn a_transaction_is_described_with_its_clock_its_timeout_and_what_it_holds() {
    let v = txn_view("orders", "orders-eos").unwrap();
    assert_eq!(
        text(&txn_sentence(&v)),
        "orders-eos is Ongoing: producer 4017 epoch 2, begun at 22:13:33, 41 s into its 1 min timeout. It holds the last stable offset of orders-0 at 16,098 and of orders-1 at 16,050."
    );
    assert_eq!(
        (
            running_ms(&v.txn, v.now_ms),
            left_ms(&v.txn, v.now_ms),
            timeout_percent(&v.txn, v.now_ms)
        ),
        (41_000, 19_000, 68)
    );
}

#[test]
fn what_it_holds_back_counts_records_per_partition() {
    let v = txn_view("orders", "orders-eos").unwrap();
    assert_eq!(
        text(&holds_sentence(&v)),
        "orders-0: 6 records, 16,098 to 16,103. orders-1: 1, at 16,050."
    );
    assert_eq!(v.held(), 7);
}

#[test]
fn if_it_is_not_ended_the_sentence_names_the_time_the_markers_and_the_new_epoch() {
    let v = txn_view("orders", "orders-eos").unwrap();
    let s = text(&unended_sentence(&v));
    assert!(
        s.starts_with("At 22:14:33, 19 s from now, the broker aborts it"),
        "{s}"
    );
    for part in [
        "orders-0 (at 16,104) and orders-1 (at 16,051)",
        "the epoch goes from 2 to 3",
        "a Produce at epoch 2 is refused with INVALID_PRODUCER_EPOCH (47)",
        "skip the 7 records",
    ] {
        assert!(s.contains(part), "{part}");
    }
    let mut late = v.clone();
    late.now_ms += 60_000;
    assert!(text(&unended_sentence(&late)).starts_with("The timeout has passed"));
    assert_eq!(left_ms(&late.txn, late.now_ms), 0);
    let mut over = v;
    over.txn.state = "CompleteCommit".to_owned();
    assert_eq!(
        text(&unended_sentence(&over)),
        "It is over: nothing is left to abort."
    );
}

#[test]
fn the_page_that_says_who_is_held_counts_the_distance_in_words() {
    let v = txn_view("orders", "orders-eos").unwrap();
    assert_eq!(
        text(&holding_sentence(&v)),
        "The last stable offset is the first offset of the oldest open transaction. On orders-0 that is 16,098, six below the high watermark 16,104."
    );
}
