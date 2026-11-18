//! Acknowledgements and locks: accept, release, reject and renew; the delivery limit; locks that run out; what is refused.

use super::*;

#[test]
fn a_release_redelivers_with_one_more_delivery_and_a_reject_never_again() {
    let mut p = SharePartition::new(0, 0);
    let log = [(0, 1), (2, 3)];
    fetch(&mut p, "m", &log, 500, 0);
    acked(&mut p, "m", &[ack(0, 1, &[RELEASE]), ack(2, 3, &[REJECT])]).unwrap();
    assert_eq!(p.start, 0, "the start waits for the released records");
    assert_eq!(p.next_fetch_offset(), 0);
    let again = fetch(&mut p, "n", &log, 500, 0);
    assert_eq!(
        ranges(&again),
        [(0, 1, 2)],
        "another member, the second delivery; 2-3 are archived"
    );
    acked(&mut p, "n", &[ack(0, 1, &[ACCEPT])]).unwrap();
    assert_eq!(p.start, 4);
}

#[test]
fn a_record_released_after_its_last_delivery_is_archived() {
    let limits = Limits {
        max_delivery: 2,
        ..LIMITS
    };
    let mut p = SharePartition::new(0, 0);
    for round in 1..=2 {
        let from = p.next_fetch_offset();
        let got = p.acquire(
            &ask("m", 500),
            &Fetched {
                from,
                spans: &spans(&[(0, 0)]),
                aborted: &[],
            },
            0,
            &limits,
        );
        assert_eq!(ranges(&got), [(0, 0, round)]);
        p.acknowledge("m", &[ack(0, 0, &[RELEASE])], 0, &limits)
            .unwrap();
        p.advance();
    }
    assert_eq!(
        (p.start, p.cached.len()),
        (1, 0),
        "archived at the limit, and done with"
    );
}

#[test]
fn a_lock_that_runs_out_makes_the_records_available_again_at_the_same_delivery_count() {
    let mut p = SharePartition::new(0, 0);
    fetch(&mut p, "m", &[(0, 2)], 500, 1_000);
    assert_eq!(p.next_deadline(), Some(31_000));
    assert!(!p.expire(30_999, &LIMITS).changed, "not yet");
    let lapsed = p.expire(31_000, &LIMITS);
    assert!(lapsed.changed && lapsed.released);
    assert_eq!(p.next_deadline(), None, "nothing held");
    let again = fetch(&mut p, "n", &[(0, 2)], 500, 40_000);
    assert_eq!(ranges(&again), [(0, 2, 2)]);
    // The member whose lock ran out may not acknowledge what it no longer holds.
    let late = p
        .acknowledge("m", &[ack(0, 2, &[ACCEPT])], 40_000, &LIMITS)
        .unwrap_err();
    assert_eq!(late.code, E::InvalidRecordState);
    assert_eq!(late.message, "Member is not the owner of batch record");
}

#[test]
fn a_lock_that_runs_out_after_the_last_delivery_archives() {
    let limits = Limits {
        max_delivery: 2,
        ..LIMITS
    };
    let mut p = SharePartition::new(0, 0);
    for now in [0, 40_000] {
        let from = p.next_fetch_offset();
        p.acquire(
            &ask("m", 500),
            &Fetched {
                from,
                spans: &spans(&[(0, 0)]),
                aborted: &[],
            },
            now,
            &limits,
        );
        p.expire(now + 30_000, &limits);
        p.advance();
    }
    assert_eq!(p.start, 1);
}

#[test]
fn renewing_restarts_the_lock_and_moves_nothing_else() {
    let mut p = SharePartition::new(0, 0);
    fetch(&mut p, "m", &[(0, 0)], 500, 0);
    let renewed = p
        .acknowledge("m", &[ack(0, 0, &[RENEW])], 20_000, &LIMITS)
        .unwrap();
    assert!(!renewed.changed, "nothing to store");
    assert_eq!(p.next_deadline(), Some(50_000));
    let off = Limits {
        renew: false,
        ..LIMITS
    };
    let refused = p
        .acknowledge("m", &[ack(0, 0, &[RENEW])], 0, &off)
        .unwrap_err();
    assert_eq!(
        refused.message,
        "Renewing acquisition locks is not enabled for the group."
    );
}

#[test]
fn per_offset_types_and_gaps_split_a_batch() {
    let mut p = SharePartition::new(0, 0);
    fetch(&mut p, "m", &[(0, 3)], 500, 0);
    acked(&mut p, "m", &[ack(0, 3, &[ACCEPT, GAP, RELEASE, REJECT])]).unwrap();
    assert!(p.cached[&0].is_each());
    assert_eq!(
        p.start, 2,
        "accepted and a gap are done with; the released one is not"
    );
    let again = fetch(&mut p, "m", &[(0, 3)], 500, 0);
    assert_eq!(ranges(&again), [(2, 2, 2)], "only the released one");
}

#[test]
fn acknowledging_what_is_not_held_is_refused_in_kafkas_words_and_changes_nothing() {
    let mut p = SharePartition::new(0, 0);
    fetch(&mut p, "m", &[(0, 4)], 500, 0);
    let before = p.clone();
    let cases: [(&str, AckBatch, E, &str); 5] = [
        (
            "x",
            ack(0, 4, &[ACCEPT]),
            E::InvalidRecordState,
            "Member is not the owner of batch record",
        ),
        (
            "m",
            ack(5, 6, &[ACCEPT]),
            E::InvalidRequest,
            "Batch record not found. The first offset in request is past acquired records.",
        ),
        (
            "m",
            ack(3, 6, &[ACCEPT]),
            E::InvalidRequest,
            "Batch record not found. The last offset in request is past acquired records.",
        ),
        (
            "x",
            ack(1, 2, &[ACCEPT]),
            E::InvalidRecordState,
            "Member is not the owner of batch record",
        ),
        (
            "m",
            ack(0, 0, &[9]),
            E::InvalidRequest,
            "Invalid acknowledge type: [9]",
        ),
    ];
    for (member, batch, code, words) in cases {
        let e = p.acknowledge(member, &[batch], 0, &LIMITS).unwrap_err();
        assert_eq!((e.code, e.message.as_str()), (code, words));
        p = before.clone();
    }
    // A later batch that fails undoes nothing on its own: the caller puts the whole partition back.
    p.acknowledge(
        "m",
        &[ack(0, 0, &[ACCEPT]), ack(9, 9, &[ACCEPT])],
        0,
        &LIMITS,
    )
    .unwrap_err();
    p = before.clone();
    acked(&mut p, "m", &[ack(1, 1, &[ACCEPT])]).unwrap();
    assert_eq!(p.start, 0, "offset 0 is still held");
    let twice = p
        .acknowledge("m", &[ack(1, 1, &[ACCEPT])], 0, &LIMITS)
        .unwrap_err();
    assert_eq!(
        twice.message,
        "The offset cannot be acknowledged. The offset is not acquired."
    );
    let empty = SharePartition::new(0, 0)
        .acknowledge("m", &[ack(0, 0, &[ACCEPT])], 0, &LIMITS)
        .unwrap_err();
    assert_eq!(
        (empty.code, empty.message.as_str()),
        (
            E::InvalidRecordState,
            "Batch record not found. The request batch offsets are not found in the cache."
        )
    );
}
