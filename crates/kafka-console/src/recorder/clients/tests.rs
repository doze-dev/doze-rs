use kafka_wire::messages::ApiKey;

use super::*;

#[test]
fn a_connection_is_a_row_until_it_closes_and_is_not_kept_past_the_cap() {
    let mut t = Conns::default();
    t.open(1, Some("127.0.0.1:50402".parse().unwrap()), 10);
    assert_eq!(t.len(), 1);
    t.close(1);
    assert_eq!(t.len(), 0);
    for i in 0..(MOST as u64 + 10) {
        t.open(i, None, 0);
    }
    assert_eq!(t.len(), MOST);
}

#[test]
fn who_is_shared_while_nothing_changes_and_new_when_the_client_or_login_does() {
    let mut t = Conns::default();
    t.open(1, Some("127.0.0.1:1".parse().unwrap()), 0);
    let a = t.who(
        1,
        "orders",
        "User:ANONYMOUS",
        Some(("sarama".into(), "1.4".into())),
    );
    let b = t.who(1, "orders", "User:ANONYMOUS", None);
    assert!(Arc::ptr_eq(&a, &b), "no allocation for the usual request");
    assert_eq!(b.software.as_deref(), Some("sarama"));
    assert_eq!(b.peer.as_deref(), Some("127.0.0.1:1"));
    let c = t.who(1, "orders", "User:alice", None);
    assert!(!Arc::ptr_eq(&a, &c));
    assert_eq!(&*c.principal, "User:alice");
    assert_eq!(
        c.software.as_deref(),
        Some("sarama"),
        "the software is remembered"
    );
}

#[test]
fn what_a_connection_has_used_is_counted_with_the_highest_version() {
    let mut t = Conns::default();
    t.open(1, None, 0);
    t.note(1, ApiKey::Produce, 7, false, 5);
    t.note(1, ApiKey::Produce, 9, true, 6);
    let (_, c) = t.iter().next().unwrap();
    assert_eq!((c.requests, c.errors, c.last_ms), (2, 1, 6));
    assert_eq!(c.apis[&(ApiKey::Produce as i16)], (9, 2));
}

#[test]
fn a_connection_the_table_does_not_hold_still_gets_a_who() {
    let mut t = Conns::default();
    let w = t.who(99, &"c".repeat(500), "User:ANONYMOUS", None);
    assert!(w.client.len() <= crate::recorder::clip::NAME);
}
