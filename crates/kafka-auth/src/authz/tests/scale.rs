//! SEC-400 (was SEC-D82): a check looks up the prefixes of the name, it does not scan the PREFIXED ACLs of the
//! type. The two model tests (`authz_model`, `authz_independent`) hold the answers; these hold the cost and the
//! edges of the lookup by length.

use std::time::{Duration, Instant};

use super::*;

fn prefix(name: &str, who: &str, op: i8, perm: i8) -> Acl {
    acl(TOPIC, name, PREFIXED, who, op, perm)
}

#[test]
fn a_check_among_fifty_thousand_prefixes_does_not_scan_them() {
    let r = rules(false, &[]);
    // Fifty thousand prefixes of one length, for another principal, plus the one that matters.
    let mut acls: Vec<Acl> = (0..50_000)
        .map(|i| prefix(&format!("p{i:05}-"), "User:bob", READ, ALLOW))
        .collect();
    acls.push(prefix("orders-", "User:alice", READ, ALLOW));
    let set = AclSet::new(acls);
    let who = Subject {
        principal: "User:alice",
        host: "10.0.0.1",
    };
    let start = Instant::now();
    let mut allowed = 0;
    for i in 0..500 {
        let name = format!("orders-{i}");
        let topic = Resource {
            kind: ResourceType::Topic,
            name: &name,
        };
        if r.authorize(&set, who, topic, Operation::Read) {
            allowed += 1;
        }
    }
    let spent = start.elapsed();
    assert_eq!(allowed, 500);
    // A scan is 25 million prefix tests here (seconds in a test build); the lookup is 500 hash probes.
    assert!(
        spent < Duration::from_secs(1),
        "500 checks among 50,000 PREFIXED ACLs took {spent:?}: the PREFIXED ACLs are being scanned"
    );
}

#[test]
fn the_lookup_by_length_finds_every_covering_prefix_and_no_other() {
    let r = rules(false, &[]);
    let set = [
        prefix("", "User:alice", READ, ALLOW),
        prefix("ö", "User:alice", WRITE, ALLOW),
        prefix("ab", "User:alice", DESCRIBE, ALLOW),
        prefix("abc-long-prefix", "User:alice", DELETE, ALLOW),
        prefix("ab", "User:alice", ALTER, DENY),
        prefix("a", "User:alice", ALTER, ALLOW),
    ];
    // The empty prefix covers every name.
    assert!(topic_can(&r, &set, "User:alice", "x", READ));
    // A multi-byte prefix: a length that falls inside a character of the name is no prefix of it.
    assert!(topic_can(&r, &set, "User:alice", "öl", WRITE));
    assert!(!topic_can(&r, &set, "User:alice", "ab", WRITE));
    assert!(topic_can(&r, &set, "User:alice", "abz", DESCRIBE));
    // A prefix longer than the name does not cover it; the same length as the name does.
    assert!(!topic_can(&r, &set, "User:alice", "abc", DELETE));
    assert!(topic_can(&r, &set, "User:alice", "abc-long-prefix", DELETE));
    // A DENY at one length wins over an ALLOW at a shorter one.
    assert!(topic_can(&r, &set, "User:alice", "a1", ALTER));
    assert!(!topic_can(&r, &set, "User:alice", "ab1", ALTER));
}
