//! The share-partition's rules (docs/port/behaviours/share-groups.md, SG-30 to SG-49), with a number for a clock: Kafka
//! 4.4.0's `SharePartition`.

mod acks;
mod stored;

use std::sync::Arc;

use kafka_meta::{ShareBatch, ShareRecordState, ShareState};
use kafka_wire::ResponseError as E;

use super::ack::{ACCEPT, GAP, REJECT, RELEASE, RENEW};
use super::*;

const LIMITS: Limits = Limits {
    max_delivery: 5,
    max_in_flight: 2000,
    lock_ms: 30_000,
    renew: true,
};

fn span(base: i64, last: i64) -> Span {
    Span {
        base,
        last,
        producer: None,
        transactional: false,
        abort_marker: false,
    }
}

fn spans(batches: &[(i64, i64)]) -> Vec<Span> {
    batches.iter().map(|(b, l)| span(*b, *l)).collect()
}

fn ask(member: &str, max_records: i64) -> AcquireAsk {
    AcquireAsk {
        member: Arc::from(member),
        record_limit: false,
        batch_size: 500,
        max_records,
        read_committed: false,
    }
}

fn ranges(a: &Acquired) -> Vec<(i64, i64, i16)> {
    a.ranges
        .iter()
        .map(|r| (r.first, r.last, r.delivery_count))
        .collect()
}

fn ack(first: i64, last: i64, types: &[i8]) -> AckBatch {
    AckBatch {
        first,
        last,
        types: types.to_vec(),
    }
}

/// Acquires what a fetch from the next fetch offset reads of `log` (batches as (base, last)).
fn fetch(p: &mut SharePartition, member: &str, log: &[(i64, i64)], max: i64, now: i64) -> Acquired {
    let from = p.next_fetch_offset();
    let read: Vec<(i64, i64)> = log
        .iter()
        .copied()
        .filter(|(_, last)| *last >= from)
        .collect();
    p.acquire(
        &ask(member, max),
        &Fetched {
            from,
            spans: &spans(&read),
            aborted: &[],
        },
        now,
        &LIMITS,
    )
}

/// Acknowledges and moves the start on, as the broker does after the write.
fn acked(p: &mut SharePartition, member: &str, batches: &[AckBatch]) -> Result<(), AckError> {
    p.acknowledge(member, batches, 0, &LIMITS)?;
    p.advance();
    Ok(())
}

#[test]
fn new_records_are_acquired_whole_batch_by_batch_and_cut_at_the_batch_size() {
    let mut p = SharePartition::new(0, 0);
    let log = [(0, 2), (3, 5), (6, 9)];
    let mut a = ask("m", 500);
    a.batch_size = 4;
    let got = p.acquire(
        &a,
        &Fetched {
            from: 0,
            spans: &spans(&log),
            aborted: &[],
        },
        0,
        &LIMITS,
    );
    assert_eq!(
        ranges(&got),
        [(0, 5, 1), (6, 9, 1)],
        "cut where a batch begins at least 4 on"
    );
    assert_eq!((got.count, p.start, p.end, p.in_flight()), (10, 0, 9, 10));
    assert_eq!(p.next_fetch_offset(), 10);
    // Nothing more to read: nothing acquired, and what is held is not acquired twice.
    assert!(fetch(&mut p, "n", &log, 500, 0).ranges.is_empty());
}

#[test]
fn the_record_limit_is_soft_in_batches_and_exact_in_record_limit_mode() {
    let log = [(0, 4), (5, 9)];
    let mut p = SharePartition::new(0, 0);
    let got = p.acquire(
        &ask("m", 3),
        &Fetched {
            from: 0,
            spans: &spans(&log),
            aborted: &[],
        },
        0,
        &LIMITS,
    );
    assert_eq!(
        ranges(&got),
        [(0, 4, 1)],
        "the whole batch holding the third record"
    );
    let mut p = SharePartition::new(0, 0);
    let mut exact = ask("m", 3);
    exact.record_limit = true;
    let got = p.acquire(
        &exact,
        &Fetched {
            from: 0,
            spans: &spans(&log),
            aborted: &[],
        },
        0,
        &LIMITS,
    );
    assert_eq!(ranges(&got), [(0, 2, 1)], "exactly three");
    assert_eq!(p.end, 4, "the batch is in flight to its end");
    assert_eq!(p.next_fetch_offset(), 3, "the rest of it is fetched next");
    let rest = p.acquire(
        &ask("n", 10),
        &Fetched {
            from: 3,
            spans: &spans(&log),
            aborted: &[],
        },
        0,
        &LIMITS,
    );
    assert_eq!(ranges(&rest), [(3, 4, 1), (5, 9, 1)]);
}

#[test]
fn an_accepted_run_moves_the_start_and_lets_go_of_whole_batches() {
    let mut p = SharePartition::new(0, 0);
    let log = [(0, 4), (5, 9)];
    let mut a = ask("m", 500);
    a.batch_size = 5;
    p.acquire(
        &a,
        &Fetched {
            from: 0,
            spans: &spans(&log),
            aborted: &[],
        },
        0,
        &LIMITS,
    );
    acked(&mut p, "m", &[ack(0, 2, &[ACCEPT])]).unwrap();
    assert_eq!(p.start, 3, "within the first batch");
    assert_eq!(p.cached.len(), 2, "a batch only part done is kept");
    acked(&mut p, "m", &[ack(3, 4, &[ACCEPT]), ack(5, 9, &[ACCEPT])]).unwrap();
    assert_eq!(
        (p.start, p.end, p.cached.len(), p.in_flight()),
        (10, 10, 0, 0)
    );
    assert_eq!(p.next_fetch_offset(), 10);
}

#[test]
fn acquisition_stops_at_the_in_flight_limit_but_released_records_inside_it_are_taken_again() {
    let limits = Limits {
        max_in_flight: 4,
        ..LIMITS
    };
    let mut p = SharePartition::new(0, 0);
    let log = [(0, 3), (4, 7)];
    let from = p.next_fetch_offset();
    let got = p.acquire(
        &ask("m", 500),
        &Fetched {
            from,
            spans: &spans(&log),
            aborted: &[],
        },
        0,
        &limits,
    );
    assert_eq!(ranges(&got), [(0, 3, 1)], "four in flight");
    assert!(!p.can_acquire(&limits));
    p.acknowledge("m", &[ack(1, 1, &[RELEASE])], 0, &limits)
        .unwrap();
    assert!(
        p.can_acquire(&limits),
        "one available inside the in-flight span"
    );
    let from = p.next_fetch_offset();
    let again = p.acquire(
        &ask("n", 500),
        &Fetched {
            from,
            spans: &spans(&log),
            aborted: &[],
        },
        0,
        &limits,
    );
    assert_eq!(
        ranges(&again),
        [(1, 1, 2)],
        "nothing past the end while at the limit"
    );
}

#[test]
fn a_record_delivered_often_is_delivered_with_fewer_others_and_on_its_last_delivery_alone() {
    let limits = Limits {
        max_delivery: 6,
        ..LIMITS
    };
    let mut p = SharePartition::new(0, 0);
    let log = [(0, 7)];
    let mut counts = Vec::new();
    for _ in 0..6 {
        let from = p.next_fetch_offset();
        let got = p.acquire(
            &ask("m", 500),
            &Fetched {
                from,
                spans: &spans(&log),
                aborted: &[],
            },
            0,
            &limits,
        );
        counts.push(ranges(&got));
        let (first, last) = (got.ranges[0].first, got.ranges.last().unwrap().last);
        p.acknowledge("m", &[ack(first, last, &[RELEASE])], 0, &limits)
            .unwrap();
    }
    // A batch whose count before this delivery is at least half the limit (3 of 6) is throttled: 8 >> 1 = 4 records,
    // then 8 >> 2 = 2; the last delivery goes alone.
    assert_eq!(counts[0], [(0, 7, 1)]);
    assert_eq!(counts[1], [(0, 7, 2)]);
    assert_eq!(counts[2], [(0, 7, 3)]);
    assert_eq!(counts[3], [(0, 3, 4)]);
    assert_eq!(counts[4], [(0, 1, 5)]);
    assert_eq!(counts[5], [(0, 0, 6)], "its last delivery, alone");
}

#[test]
fn under_read_committed_an_aborted_transactions_records_are_archived_not_delivered() {
    let mut p = SharePartition::new(0, 0);
    let txn = |base, last| Span {
        producer: Some(7),
        transactional: true,
        ..span(base, last)
    };
    let marker = Span {
        abort_marker: true,
        ..txn(4, 4)
    };
    let log = [span(0, 1), txn(2, 3), marker, span(5, 6)];
    let mut a = ask("m", 500);
    a.read_committed = true;
    let aborted = [Aborted {
        producer: 7,
        first: 2,
    }];
    let got = p.acquire(
        &a,
        &Fetched {
            from: 0,
            spans: &log,
            aborted: &aborted,
        },
        0,
        &LIMITS,
    );
    assert_eq!(
        ranges(&got),
        [(0, 1, 1), (4, 6, 1)],
        "the marker is acquired, to come back as a gap"
    );
    acked(
        &mut p,
        "m",
        &[ack(0, 1, &[ACCEPT]), ack(4, 6, &[ACCEPT, GAP, ACCEPT])],
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        p.start, 7,
        "the archived transaction does not hold the start"
    );
}

#[test]
fn a_log_start_past_the_start_archives_what_is_available_before_it() {
    let mut p = SharePartition::new(0, 0);
    fetch(&mut p, "m", &[(0, 3)], 500, 0);
    p.acknowledge("m", &[ack(0, 1, &[RELEASE])], 0, &LIMITS)
        .unwrap();
    p.log_start_moved(2);
    assert_eq!(p.start, 2);
    assert_eq!(
        p.next_fetch_offset(),
        4,
        "0 and 1 are archived, 2 and 3 still held"
    );
    let mut empty = SharePartition::new(0, 0);
    empty.log_start_moved(7);
    assert_eq!((empty.start, empty.end), (7, 7));
}

#[test]
fn a_member_that_goes_lets_go_of_what_it_holds_and_only_that() {
    let mut p = SharePartition::new(0, 0);
    fetch(&mut p, "a", &[(0, 1)], 500, 0);
    fetch(&mut p, "b", &[(0, 1), (2, 3)], 500, 0);
    let let_go = p.release_member("a", &LIMITS);
    assert!(let_go.changed && let_go.released);
    assert_eq!(p.cached[&2].track.held_by("b"), 1);
    assert_eq!(
        ranges(&fetch(&mut p, "c", &[(0, 1), (2, 3)], 500, 0)),
        [(0, 1, 2)]
    );
}

#[test]
fn the_state_machine_moves_a_record_only_as_kafka_does() {
    let r = |state| Rec::new(state, 1);
    let to = |from: RecordState, to: RecordState| {
        r(from).moved(to, Count::Same, 5, None).map(|r| r.state)
    };
    use RecordState::{Acknowledged, Acquired, Archived, Available};
    assert_eq!(to(Available, Acquired), Some(Acquired));
    for target in [Available, Acknowledged, Archived] {
        assert_eq!(to(Available, target), None, "available to {target:?}");
        assert_eq!(to(Acknowledged, target), None);
        assert_eq!(to(Archived, target), None);
    }
    assert_eq!(to(Acquired, Acquired), None, "the same state");
    assert_eq!(to(Acquired, Acknowledged), Some(Acknowledged));
    let at_limit = Rec::new(Acquired, 5);
    assert_eq!(
        at_limit
            .moved(Available, Count::Same, 5, None)
            .map(|r| r.state),
        Some(Archived)
    );
    assert_eq!(
        Rec::new(Available, 1)
            .moved(Acquired, Count::Increase, 5, None)
            .map(|r| r.count),
        Some(2)
    );
}
