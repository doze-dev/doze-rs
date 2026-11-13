use super::*;
use crate::fixtures_log::txn_view;

fn text(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|s| match s {
            Span::Text(t) | Span::Mono(t) | Span::Strong(t) => t.as_str(),
        })
        .collect()
}

#[test]
fn a_transaction_is_described_with_its_clock_its_timeout_and_what_it_holds() {
    let v = txn_view("orders", "orders-eos").unwrap();
    assert_eq!(
        text(&txn_sentence(&v)),
        "orders-eos is Ongoing: producer 4017 epoch 2, begun at 22:13:33, 41 s into its 60 s timeout. It holds the last stable offset of orders-0 at 16,098 and of orders-1 at 16,050."
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
fn the_page_that_says_who_is_held_counts_the_distance_in_words() {
    let v = txn_view("orders", "orders-eos").unwrap();
    assert_eq!(
        text(&holding_sentence(&v)),
        "The last stable offset is the first offset of the oldest open transaction. On orders-0 that is 16,098, six below the high watermark 16,104."
    );
}
