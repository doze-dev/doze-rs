use std::sync::Arc;

use kafka_wire::messages::ApiKey;

use super::*;
use crate::recorder::errors::Class;
use crate::recorder::facts::Facts;
use crate::recorder::row::Who;

fn who() -> Arc<Who> {
    Arc::new(Who {
        client: "app".into(),
        software: None,
        version: None,
        principal: "User:ANONYMOUS".into(),
        peer: None,
    })
}

fn row(at: i64, key: u64) -> Row {
    Row {
        id: 0,
        touched: 0,
        first_ms: at,
        last_ms: at,
        count: 1,
        conn: 1,
        who: who(),
        api: ApiKey::Heartbeat,
        version: 4,
        correlation: 1,
        facts: Box::new(Facts::default()),
        class: Class::Ok,
        closed: None,
        code: 0,
        request_bytes: 10,
        response_bytes: 10,
        tooks: vec![1000],
        idle: true,
        fold: key,
        lab: None,
    }
}

#[test]
fn an_idle_ring_holds_nothing() {
    let ring = Ring::default();
    assert_eq!((ring.len(), ring.bytes(), ring.requests()), (0, 0, 0));
    assert_eq!(ring.newest_id(), 0);
}

#[test]
fn repeats_within_the_window_fold_into_one_row_and_a_later_one_does_not() {
    let mut ring = Ring::default();
    let a = ring.record(row(0, 7), Fold::Poll);
    let b = ring.record(row(5_000, 7), Fold::Poll);
    assert_eq!(a, b);
    assert_eq!(ring.len(), 1);
    assert_eq!(ring.get(a).unwrap().count, 2);
    assert_eq!(ring.get(a).unwrap().last_ms, 5_000);
    assert_eq!(ring.requests(), 2);
    // Past ten minutes a heartbeat starts a row of its own.
    let c = ring.record(row(5_000 + FOLD_POLL_MS + 1, 7), Fold::Poll);
    assert_ne!(a, c);
    assert_eq!(ring.len(), 2);
}

#[test]
fn a_request_that_is_not_a_poll_folds_only_in_a_stream() {
    let mut ring = Ring::default();
    let a = ring.record(row(0, 9), Fold::Stream);
    assert_eq!(ring.record(row(1_500, 9), Fold::Stream), a);
    let c = ring.record(row(1_500 + FOLD_STREAM_MS + 1, 9), Fold::Stream);
    assert_ne!(a, c);
}

#[test]
fn rows_that_never_fold_and_rows_with_other_keys_stand_alone() {
    let mut ring = Ring::default();
    ring.record(row(0, 1), Fold::Never);
    ring.record(row(1, 1), Fold::Never);
    ring.record(row(2, 2), Fold::Poll);
    ring.record(row(3, 3), Fold::Poll);
    assert_eq!(ring.len(), 4);
}

#[test]
fn a_fold_bumps_the_version_so_since_can_see_it() {
    let mut ring = Ring::default();
    ring.record(row(0, 7), Fold::Poll);
    let v = ring.version();
    ring.record(row(10, 7), Fold::Poll);
    assert!(ring.version() > v);
    assert_eq!(ring.iter().next().unwrap().touched, ring.version());
}

#[test]
fn the_ring_never_holds_more_than_its_rows() {
    let mut ring = Ring::default();
    for i in 0..(ROWS as i64 + 500) {
        ring.record(row(i, 0), Fold::Never);
    }
    assert_eq!(ring.len(), ROWS);
    assert!(ring.get(0).is_none(), "the oldest were dropped");
    assert!(ring.get(ROWS as u64 + 499).is_some());
}

#[test]
fn the_ring_never_holds_more_than_its_byte_budget() {
    let mut ring = Ring::default();
    for i in 0..400 {
        let mut r = row(i, 0);
        for t in 0..20 {
            r.facts.topic(&format!("{t}{}", "x".repeat(126)), 0..8);
        }
        ring.record(r, Fold::Never);
    }
    assert!(ring.bytes() <= BUDGET, "{} over {BUDGET}", ring.bytes());
    assert!(
        ring.high_water() <= BUDGET + 20_000,
        "never far over: {}",
        ring.high_water()
    );
    assert!(ring.len() < 400, "the budget, not the count, ended it");
    assert!(ring.len() > 1);
}

#[test]
fn the_byte_count_is_what_the_rows_weigh() {
    let mut ring = Ring::default();
    for i in 0..50 {
        ring.record(row(i, 0), Fold::Never);
    }
    let sum: usize = ring.iter().map(Row::weight).sum();
    assert_eq!(ring.bytes(), sum);
    ring.record(row(60, 5), Fold::Poll);
    ring.record(row(61, 5), Fold::Poll);
    let sum: usize = ring.iter().map(Row::weight).sum();
    assert_eq!(ring.bytes(), sum, "a fold keeps the count right too");
}

#[test]
fn the_fold_index_forgets_a_row_that_was_dropped() {
    let mut ring = Ring::default();
    ring.record(row(0, 42), Fold::Poll);
    for i in 1..=(ROWS as i64) {
        ring.record(row(i, 0), Fold::Never);
    }
    assert_eq!(ring.len(), ROWS);
    // The first row is gone; the same key now starts a new row and does not reach for the dead one.
    let before = ring.len();
    ring.record(row(ROWS as i64 + 5, 42), Fold::Poll);
    assert_eq!(ring.len(), before);
    assert!(ring.iter().any(|r| r.fold == 42));
}

#[test]
fn the_took_samples_are_the_last_sixty_four() {
    let mut ring = Ring::default();
    for i in 0..100 {
        let mut r = row(i, 7);
        r.tooks = vec![u32::try_from(i).unwrap()];
        ring.record(r, Fold::Poll);
    }
    let r = ring.iter().next().unwrap();
    assert_eq!(r.count, 100);
    assert_eq!(r.tooks.len(), crate::recorder::row::SAMPLES);
    assert_eq!(r.tooks.last(), Some(&99));
    assert_eq!(r.median(), 68);
}

#[test]
fn the_tape_is_in_the_order_of_its_times_even_when_the_clock_steps_back() {
    let mut ring = Ring::default();
    ring.record(row(10_000, 0), Fold::Never);
    // A clock that stepped back (or two threads that read it a hair apart): the row still sits above the older one.
    ring.record(row(9_000, 0), Fold::Never);
    ring.record(row(12_000, 0), Fold::Never);
    let times: Vec<i64> = ring.iter().map(|r| r.first_ms).collect();
    assert_eq!(times, vec![10_000, 10_000, 12_000]);
    assert!(ring.iter().all(|r| r.last_ms >= r.first_ms));
}

#[test]
fn a_folded_row_keeps_its_place_and_its_first_time_while_its_last_time_grows() {
    let mut ring = Ring::default();
    let a = ring.record(row(1_000, 7), Fold::Poll);
    let b = ring.record(row(2_000, 0), Fold::Never);
    let again = ring.record(row(3_000, 7), Fold::Poll);
    assert_eq!(again, a, "the repeat folds into the first row");
    let order: Vec<u64> = ring.iter().map(|r| r.id).collect();
    assert_eq!(order, vec![a, b], "the folded row did not move");
    let r = ring.get(a).unwrap();
    assert_eq!((r.first_ms, r.last_ms, r.count), (1_000, 3_000, 2));
}
