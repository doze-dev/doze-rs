//! Hosts, names and namespaces in the decision (kafka-semantics 1.2 and 1.3).

use super::*;

#[test]
fn a_host_is_star_or_the_same_text() {
    let r = rules(false, &[]);
    let mut a = allow("t", "User:a", READ);
    a.host = "10.0.0.1".into();
    let set = [a];
    assert!(can(&r, &set, ("User:a", "10.0.0.1"), (TOPIC, "t"), READ)); // SAD:492
    assert!(!can(&r, &set, ("User:a", "10.0.0.2"), (TOPIC, "t"), READ));
    assert!(!can(&r, &set, ("User:a", "10.0.0.10"), (TOPIC, "t"), READ)); // no prefix, no CIDR
    let mut d = deny("t", "User:a", READ);
    d.host = "10.0.0.2".into();
    let set = [allow("t", "User:a", READ), d];
    assert!(can(&r, &set, ("User:a", "10.0.0.1"), (TOPIC, "t"), READ)); // the deny is for another host
    assert!(!can(&r, &set, ("User:a", "10.0.0.2"), (TOPIC, "t"), READ));
}

#[test]
fn literal_prefixed_and_wildcard_names() {
    let r = rules(false, &[]);
    // LITERAL names only the resource; a literal that is a prefix of the name does not cover it (SAD:398-405).
    let set = [allow("foo", "User:a", READ)];
    assert!(topic_can(&r, &set, "User:a", "foo", READ));
    assert!(!topic_can(&r, &set, "User:a", "foobar", READ));
    // PREFIXED covers the names it starts, itself included.
    let set = [acl(TOPIC, "foo", PREFIXED, "User:a", READ, ALLOW)];
    assert!(topic_can(&r, &set, "User:a", "foo", READ));
    assert!(topic_can(&r, &set, "User:a", "foobar", READ));
    assert!(!topic_can(&r, &set, "User:a", "fo", READ));
    assert!(!topic_can(&r, &set, "User:a", "xfoo", READ));
    // LITERAL `*` covers every name of the type, and only LITERAL does: PREFIXED `*` is a prefix of names that
    // start with an asterisk.
    let set = [allow("*", "User:a", READ)];
    assert!(topic_can(&r, &set, "User:a", "anything", READ));
    assert!(topic_can(&r, &set, "User:a", "*", READ));
    let set = [acl(TOPIC, "*", PREFIXED, "User:a", READ, ALLOW)];
    assert!(!topic_can(&r, &set, "User:a", "anything", READ));
    assert!(topic_can(&r, &set, "User:a", "*starred", READ));
    // Kafka's walk skips the LITERAL `*` for a resource that starts with an asterisk but is not just `*`: it reads
    // as a literal that is only a prefix of the name (SAD:398-405). Kept, for parity: the default applies.
    let set = [allow("*", "User:a", READ)];
    assert!(!topic_can(&r, &set, "User:a", "*x", READ));
    assert!(topic_can(&rules(true, &[]), &set, "User:b", "*x", READ));
    assert!(!topic_can(&rules(true, &[]), &set, "User:b", "x", READ));
    // Case counts.
    assert!(!topic_can(
        &r,
        &[allow("Foo", "User:a", READ)],
        "User:a",
        "foo",
        READ
    ));
}

#[test]
fn the_cluster_resource_has_its_own_namespace() {
    let r = rules(false, &[]);
    let set = [acl(CLUSTER, CLUSTER_NAME, LITERAL, "User:a", ALTER, ALLOW)];
    assert!(can(
        &r,
        &set,
        ("User:a", "h"),
        (CLUSTER, CLUSTER_NAME),
        ALTER
    ));
    let on_cluster = |op| can(&r, &set, ("User:a", "h"), (CLUSTER, CLUSTER_NAME), op);
    assert!(on_cluster(DESCRIBE)); // ALTER implies DESCRIBE
    assert!(!can(
        &r,
        &set,
        ("User:a", "h"),
        (TOPIC, CLUSTER_NAME),
        ALTER
    ));
}

#[test]
fn a_type_that_cannot_be_stored_is_uncovered() {
    let r = rules(true, &[]);
    assert!(can(&r, &[], ("User:a", "h"), (0, "x"), READ));
    assert!(can(&r, &[], ("User:a", "h"), (99, "x"), READ));
    assert!(can(&r, &[], ("User:a", "h"), (-3, "x"), READ));
}
