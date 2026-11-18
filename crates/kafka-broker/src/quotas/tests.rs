use kafka_meta::QuotaName;

use super::mutations::Exhausted;
use super::*;

const T: i64 = 1_700_000_000_000;

fn name(s: &str) -> Option<QuotaName> {
    Some(QuotaName::Named(s.to_owned()))
}

fn entity(user: Option<QuotaName>, client: Option<QuotaName>) -> QuotaEntity {
    QuotaEntity {
        user,
        client_id: client,
        ip: None,
    }
}

fn quotas(rows: &[(QuotaEntity, &str, f64)]) -> Quotas {
    let mut set: BTreeMap<QuotaEntity, BTreeMap<String, f64>> = BTreeMap::new();
    for (e, key, v) in rows {
        set.entry(e.clone())
            .or_default()
            .insert((*key).to_owned(), *v);
    }
    let q = Quotas::default();
    q.replace(set);
    q
}

fn who<'a>(user: &'a str, id: &'a str) -> Client<'a> {
    Client { user, id }
}

/// The limit a client is held to for `kind`, as its meter's tags find it.
fn limit_of(q: &Quotas, kind: QuotaKind, c: Client<'_>) -> Option<f64> {
    let state = q.lock();
    let k = state.kinds.get(kind.index()).unwrap();
    let (u, id) = k.tags(c);
    k.limits.limit(u, id)
}

const P: &str = "producer_byte_rate";

#[test]
fn the_most_specific_level_that_matches_applies_in_kafkas_order() {
    let d = Some(QuotaName::Default);
    let q = quotas(&[
        (entity(name("alice"), name("c1")), P, 1.0),
        (entity(name("alice"), d.clone()), P, 2.0),
        (entity(name("alice"), None), P, 3.0),
        (entity(d.clone(), name("c1")), P, 4.0),
        (entity(d.clone(), d.clone()), P, 5.0),
        (entity(d.clone(), None), P, 6.0),
        (entity(None, name("c1")), P, 7.0),
        (entity(None, d), P, 8.0),
    ]);
    let k = QuotaKind::Produce;
    assert_eq!(limit_of(&q, k, who("alice", "c1")), Some(1.0));
    assert_eq!(limit_of(&q, k, who("alice", "c2")), Some(2.0));
    assert_eq!(limit_of(&q, k, who("bob", "c1")), Some(4.0));
    assert_eq!(limit_of(&q, k, who("bob", "c2")), Some(5.0));
}

#[test]
fn each_level_alone_is_found_with_its_default_behind_it() {
    let k = QuotaKind::Produce;
    let users = quotas(&[
        (entity(name("alice"), None), P, 3.0),
        (entity(Some(QuotaName::Default), None), P, 6.0),
    ]);
    assert_eq!(limit_of(&users, k, who("alice", "x")), Some(3.0));
    assert_eq!(limit_of(&users, k, who("bob", "x")), Some(6.0));
    let clients = quotas(&[
        (entity(None, name("c1")), P, 7.0),
        (entity(None, Some(QuotaName::Default)), P, 8.0),
    ]);
    assert_eq!(limit_of(&clients, k, who("alice", "c1")), Some(7.0));
    assert_eq!(limit_of(&clients, k, who("alice", "c9")), Some(8.0));
    // A user level and a client id level together: the user's applies, then the client id's for users with none.
    let both = quotas(&[
        (entity(name("alice"), None), P, 3.0),
        (entity(None, name("c1")), P, 7.0),
    ]);
    assert_eq!(limit_of(&both, k, who("alice", "c1")), Some(3.0));
    assert_eq!(limit_of(&both, k, who("bob", "c1")), Some(7.0));
    assert_eq!(limit_of(&both, k, who("bob", "c2")), None);
}

#[test]
fn a_client_with_no_client_id_escapes_client_id_quotas_as_in_kafka() {
    let q = quotas(&[(entity(None, Some(QuotaName::Default)), P, 8.0)]);
    assert_eq!(limit_of(&q, QuotaKind::Produce, who("alice", "")), None);
}

#[test]
fn each_quota_is_its_own_and_an_address_quota_switches_nothing_on() {
    let q = quotas(&[(entity(name("alice"), None), "consumer_byte_rate", 3.0)]);
    assert_eq!(limit_of(&q, QuotaKind::Produce, who("alice", "c")), None);
    assert_eq!(limit_of(&q, QuotaKind::Fetch, who("alice", "c")), Some(3.0));
    assert!(q.enabled());
    let ip = QuotaEntity {
        user: None,
        client_id: None,
        ip: name("10.0.0.1"),
    };
    let off = quotas(&[(ip, "connection_creation_rate", 5.0)]);
    assert!(
        !off.enabled(),
        "addresses are kept and described, not metered"
    );
    assert!(!Quotas::default().enabled());
}

#[test]
fn a_user_quota_is_shared_by_every_client_id_of_the_user() {
    let q = quotas(&[(entity(name("alice"), None), P, 1000.0)]);
    // 6000 bytes from one client id and 6000 from another, in the same second: 12000 over a ten second span is 1200/s.
    assert_eq!(
        q.record(QuotaKind::Produce, who("alice", "a"), 6000.0, T),
        0
    );
    let wait = q.record(QuotaKind::Produce, who("alice", "b"), 6000.0, T + 1);
    // 12000 bytes over a span of 10 001 ms is 1199.88/s; (1199.88 - 1000) / 1000 * 10 001 ms.
    assert_eq!(wait, 1999);
    let usage = q.usage(T + 1);
    assert_eq!(usage.len(), 1);
    assert_eq!(
        (usage[0].user.as_str(), usage[0].client_id.as_str()),
        ("alice", "")
    );
}

#[test]
fn a_default_client_id_quota_meters_each_client_id_apart() {
    let q = quotas(&[(entity(None, Some(QuotaName::Default)), P, 1000.0)]);
    assert_eq!(q.record(QuotaKind::Produce, who("u", "a"), 6000.0, T), 0);
    assert_eq!(
        q.record(QuotaKind::Produce, who("u", "b"), 6000.0, T + 1),
        0
    );
    assert_eq!(q.usage(T + 1).len(), 2);
}

#[test]
fn the_request_quota_throttles_for_one_window_at_most() {
    let q = quotas(&[(entity(name("alice"), None), "request_percentage", 1.0)]);
    // A whole second of thread time is 100 percent, recorded at once: far over 1 percent.
    let wait = q.record(QuotaKind::Request, who("alice", "c"), 100.0, T);
    assert_eq!(wait, 1000);
}

#[test]
fn a_fetch_is_capped_at_what_the_span_less_one_window_allows() {
    let q = quotas(&[(entity(None, name("c")), "consumer_byte_rate", 100.0)]);
    assert_eq!(
        q.most_in_window(QuotaKind::Fetch, who("u", "c")),
        Some(1000.0)
    );
    assert_eq!(q.most_in_window(QuotaKind::Fetch, who("u", "other")), None);
    assert_eq!(q.most_in_window(QuotaKind::Produce, who("u", "c")), None);
}

#[test]
fn a_throttled_fetch_gives_its_bytes_back() {
    let q = quotas(&[(entity(None, name("c")), "consumer_byte_rate", 100.0)]);
    assert!(q.record(QuotaKind::Fetch, who("u", "c"), 5000.0, T) > 0);
    q.unrecord(QuotaKind::Fetch, who("u", "c"), 5000.0, T);
    assert_eq!(q.record(QuotaKind::Fetch, who("u", "c"), 10.0, T + 1), 0);
}

#[test]
fn strict_mutations_take_the_one_that_empties_the_bucket_and_refuse_the_next() {
    let q = quotas(&[(entity(None, name("c")), "controller_mutation_rate", 10.0)]);
    let mut m = q.mutations(who("u", "c"), true);
    // The bucket holds 11 windows' worth: 110 partitions.
    assert_eq!(m.record(100, T), Ok(()));
    assert_eq!(
        m.record(20, T),
        Ok(()),
        "10 left: taken, and the bucket is at -10"
    );
    assert_eq!(m.record(1, T), Err(Exhausted));
    assert_eq!(m.throttle_time(T), 1000, "-10 tokens at 10 a second");
    assert_eq!(
        m.throttle_time(T + 400),
        600,
        "counted down from when it was worked out"
    );
    // A new request a second later: the bucket is back at zero, so it is taken.
    let mut later = q.mutations(who("u", "c"), true);
    assert_eq!(later.record(5, T + 1000), Ok(()));
    assert_eq!(later.throttle_time(T + 1000), 0);
}

#[test]
fn permissive_mutations_are_taken_and_throttle_the_client() {
    let q = quotas(&[(entity(None, name("c")), "controller_mutation_rate", 10.0)]);
    let mut m = q.mutations(who("u", "c"), false);
    assert_eq!(m.record(130, T), Ok(()));
    assert_eq!(m.throttle_time(T), 2000);
    assert_eq!(m.record(10, T), Ok(()), "taken however far below zero");
    assert_eq!(m.throttle_time(T), 3000);
}

#[test]
fn with_no_mutation_quota_every_mutation_is_taken() {
    let q = quotas(&[(entity(None, name("c")), P, 10.0)]);
    let mut m = q.mutations(who("u", "c"), true);
    for _ in 0..100 {
        assert_eq!(m.record(1000, T), Ok(()));
    }
    assert_eq!(m.throttle_time(T), 0);
    let mut none = Mutations::unbounded();
    assert_eq!(none.record(i32::MAX, T), Ok(()));
}

#[test]
fn meters_go_when_the_last_limit_goes_and_idle_ones_are_dropped_after_an_hour() {
    let q = quotas(&[(entity(None, Some(QuotaName::Default)), P, 1000.0)]);
    q.record(QuotaKind::Produce, who("u", "a"), 1.0, T);
    q.record(QuotaKind::Produce, who("u", "b"), 1.0, T + IDLE_MS + 1);
    assert_eq!(
        q.usage(T + IDLE_MS + 1).len(),
        1,
        "a is an hour idle: dropped when b was made"
    );
    q.replace(BTreeMap::new());
    assert!(!q.enabled());
    assert!(q.usage(T).is_empty());
    assert_eq!(q.record(QuotaKind::Produce, who("u", "b"), 1e12, T), 0);
}
