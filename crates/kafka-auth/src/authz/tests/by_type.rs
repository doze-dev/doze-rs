//! `authorizeByResourceType` (kafka-semantics 1.5, `Authorizer.java` 184-293).

use super::*;

fn may(r: &Rules, set: &[Acl], who: &str, op: i8) -> bool {
    let set = AclSet::new(set.iter().cloned());
    r.authorize_by_resource_type(
        &set,
        Subject {
            principal: who,
            host: "10.0.0.1",
        },
        op,
        TOPIC,
    )
}

fn prefix_allow(name: &str, who: &str, op: i8) -> Acl {
    acl(TOPIC, name, PREFIXED, who, op, ALLOW)
}

fn prefix_deny(name: &str, who: &str, op: i8) -> Acl {
    acl(TOPIC, name, PREFIXED, who, op, DENY)
}

#[test]
fn write_on_one_topic_is_write_on_a_topic() {
    let r = rules(false, &[]);
    // The recorded case (idempotent-write-from-topic-write): alice Writes `iw`, bob has nothing.
    let set = [allow("iw", "User:alice", WRITE)];
    assert!(may(&r, &set, "User:alice", WRITE));
    assert!(!may(&r, &set, "User:bob", WRITE));
    assert!(may(
        &r,
        &[prefix_allow("i", "User:alice", WRITE)],
        "User:alice",
        WRITE
    ));
    assert!(may(
        &r,
        &[allow("*", "User:alice", WRITE)],
        "User:alice",
        WRITE
    ));
    assert!(may(&r, &[allow("iw", "User:*", WRITE)], "User:bob", WRITE));
}

#[test]
fn the_operation_is_equal_or_all_and_nothing_implies() {
    let r = rules(false, &[]);
    assert!(may(&r, &[allow("t", "User:a", ALL)], "User:a", WRITE));
    assert!(!may(&r, &[allow("t", "User:a", READ)], "User:a", WRITE));
    assert!(!may(&r, &[allow("t", "User:a", ALTER)], "User:a", DESCRIBE));
    assert!(may(
        &r,
        &[allow("t", "User:a", DESCRIBE)],
        "User:a",
        DESCRIBE
    ));
}

#[test]
fn a_deny_of_star_refuses_at_once_and_an_allow_of_star_permits() {
    let r = rules(false, &[]);
    let set = [allow("t", "User:a", WRITE), deny("*", "User:a", WRITE)];
    assert!(!may(&r, &set, "User:a", WRITE));
    // The wildcard allow is looked at after the denies.
    let set = [allow("*", "User:a", WRITE), deny("t", "User:a", WRITE)];
    assert!(may(&r, &set, "User:a", WRITE));
}

#[test]
fn an_allow_is_dominated_by_a_deny_of_its_name_or_of_a_prefix_of_it() {
    let r = rules(false, &[]);
    // A literal allow is dominated by a literal deny of the same name ...
    let set = [allow("t", "User:a", WRITE), deny("t", "User:a", WRITE)];
    assert!(!may(&r, &set, "User:a", WRITE));
    // ... but not by a literal deny of another.
    let set = [allow("t", "User:a", WRITE), deny("u", "User:a", WRITE)];
    assert!(may(&r, &set, "User:a", WRITE));
    // A prefixed deny dominates the allows it is a prefix of (itself included), literal or prefixed.
    let set = [
        allow("topic", "User:a", WRITE),
        prefix_deny("top", "User:a", WRITE),
    ];
    assert!(!may(&r, &set, "User:a", WRITE));
    let set = [
        prefix_allow("topic", "User:a", WRITE),
        prefix_deny("topic", "User:a", WRITE),
    ];
    assert!(!may(&r, &set, "User:a", WRITE));
    // A longer deny prefix does not dominate a shorter allow.
    let set = [
        prefix_allow("top", "User:a", WRITE),
        prefix_deny("topic", "User:a", WRITE),
    ];
    assert!(may(&r, &set, "User:a", WRITE));
    // One undominated allow among dominated ones is enough.
    let set = [
        allow("t", "User:a", WRITE),
        deny("t", "User:a", WRITE),
        allow("other", "User:a", WRITE),
    ];
    assert!(may(&r, &set, "User:a", WRITE));
    // A literal deny does not dominate a prefixed allow of that name.
    let set = [
        prefix_allow("t", "User:a", WRITE),
        deny("t", "User:a", WRITE),
    ];
    assert!(may(&r, &set, "User:a", WRITE));
}

#[test]
fn acls_for_other_principals_hosts_and_types_do_not_count() {
    let r = rules(false, &[]);
    assert!(!may(&r, &[allow("t", "User:b", WRITE)], "User:a", WRITE));
    let mut elsewhere = allow("t", "User:a", WRITE);
    elsewhere.host = "9.9.9.9".into();
    assert!(!may(&r, &[elsewhere], "User:a", WRITE));
    let group = acl(GROUP, "t", LITERAL, "User:a", WRITE, ALLOW);
    assert!(!may(&r, &[group], "User:a", WRITE));
    // A deny for another principal does not stand in the way.
    let set = [allow("t", "User:a", WRITE), deny("*", "User:b", WRITE)];
    assert!(may(&r, &set, "User:a", WRITE));
}

#[test]
fn a_super_user_passes_and_the_default_is_only_the_hardcoded_names() {
    let r = rules(false, &["User:admin"]);
    assert!(may(
        &r,
        &[deny("*", "User:admin", WRITE)],
        "User:admin",
        WRITE
    ));
    // With the default allowing, the name `hardcode` has no ACL and so everybody passes ...
    let open = rules(true, &[]);
    assert!(may(&open, &[], "User:a", WRITE));
    // ... unless an ACL covers `hardcode` (here the wildcard, for somebody else), and then the default is off.
    assert!(!may(&open, &[allow("*", "User:b", WRITE)], "User:a", WRITE));
    // An ACL that does not cover `hardcode` leaves the default on.
    assert!(may(&open, &[allow("t", "User:b", WRITE)], "User:a", WRITE));
}
