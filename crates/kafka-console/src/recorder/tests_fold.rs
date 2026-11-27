//! Folding as the server drives it: a repeat is added to the row it repeats before any row is built for it, and adds the same
//! counts a row of its own would have.

use kafka_server::Answer;

use super::tests::{open, play, produce};
use super::*;

#[test]
fn identical_requests_fold_into_one_row_that_sums_them_and_different_ones_stand_alone() {
    let rec = Recorder::new();
    open(&rec, 1);
    open(&rec, 2);
    let (q, a) = produce("orders", 3, 0);
    for _ in 0..3 {
        play(&rec, 1, "app", &q, Answer::Response(&a), 9);
    }
    // Another connection, another topic, another version and a failure are each a row of their own.
    play(&rec, 2, "app", &q, Answer::Response(&a), 9);
    let (other, other_a) = produce("payments", 3, 0);
    play(&rec, 1, "app", &other, Answer::Response(&other_a), 9);
    play(&rec, 1, "app", &q, Answer::Response(&a), 8);
    let (bad, bad_a) = produce("orders", 3, 87);
    play(&rec, 1, "app", &bad, Answer::Response(&bad_a), 9);
    play(&rec, 1, "app", &bad, Answer::Response(&bad_a), 9);

    let inner = rec.lock();
    assert_eq!(
        inner.ring.requests(),
        8,
        "every request is counted, folded or not"
    );
    let rows: Vec<_> = inner.ring.iter().collect();
    assert_eq!(rows.len(), 6);
    let first = rows[0];
    assert_eq!((first.conn, first.count), (1, 3));
    assert_eq!(
        (first.facts.records, first.facts.batches),
        (9, 3),
        "the folded row sums its requests' records and batches"
    );
    assert_eq!((first.request_bytes, first.response_bytes), (300, 120));
    assert_eq!(first.tooks, [1500, 1500, 1500]);
    assert_eq!(&*first.facts.topics[0].name, "orders");
    assert!(rows[1..].iter().all(|r| r.count == 1));
    assert_eq!(
        inner.ring.bytes(),
        rows.iter().map(|r| r.weight()).sum::<usize>(),
        "the byte count follows the folded row's growth"
    );
}
