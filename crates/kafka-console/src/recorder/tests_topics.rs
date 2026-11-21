//! What the topic pages learn from the tape: who wrote an abort marker (debt J238).

use kafka_server::Answer;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::*;

use super::tests::{open, play};
use super::*;
use crate::model::AbortBy;

fn end_txn(pid: i64, committed: bool) -> (Request, Response) {
    let mut q = EndTxnRequest::default();
    q.transactional_id = TransactionalId(StrBytes::from_static_str("orders-eos"));
    q.producer_id = ProducerId(pid);
    q.producer_epoch = 3;
    q.committed = committed;
    (
        Request::EndTxn(q),
        Response::EndTxn(EndTxnResponse::default()),
    )
}

#[test]
fn an_abort_is_the_producers_when_its_end_txn_is_on_the_tape_and_the_brokers_when_the_tape_reaches_past_it_without_one()
 {
    let rec = Recorder::new();
    open(&rec, 1);
    let now = crate::source::now_ms();
    let (q, a) = end_txn(4017, false);
    play(&rec, 1, "orders", &q, Answer::Response(&a), 3);
    let (q, a) = end_txn(4020, true);
    play(&rec, 1, "orders", &q, Answer::Response(&a), 3);
    assert_eq!(
        rec.abort_by(4017, now),
        Some(AbortBy::Producer {
            client: "orders".to_owned()
        })
    );
    // A commit is no abort; a marker a minute after the tape began, with no EndTxn near it: the broker wrote it.
    assert_eq!(rec.abort_by(4020, now + 60_000), Some(AbortBy::Broker));
    // A marker from before the tape: not known.
    assert_eq!(rec.abort_by(4020, now), None);
    assert_eq!(
        Recorder::new().abort_by(1, now),
        None,
        "an empty tape knows nothing"
    );
}
