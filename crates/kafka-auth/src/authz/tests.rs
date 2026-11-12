//! The decision procedure, rule by rule (docs/port/phase8/kafka-semantics.md 1.3 to 1.5). Each test names the rule
//! and the line of `StandardAuthorizerData.java` or `Authorizer.java` (Kafka 4.3.1) it comes from.

use super::operation::{
    ALL, ALTER, ALTER_CONFIGS, CREATE, DELETE, DESCRIBE, DESCRIBE_CONFIGS, IDEMPOTENT_WRITE, READ,
    WRITE,
};
use super::pattern::{LITERAL, PREFIXED};
use super::permission::{ALLOW, DENY};
use super::resource::{CLUSTER, GROUP, TOPIC};
use super::*;

mod by_type;
mod names;
mod validate;

pub(super) fn acl(kind: i8, name: &str, pat: i8, principal: &str, op: i8, perm: i8) -> Acl {
    Acl {
        resource_type: kind,
        name: name.to_owned(),
        pattern_type: pat,
        principal: principal.to_owned(),
        host: "*".to_owned(),
        operation: op,
        permission: perm,
    }
}

pub(super) fn allow(name: &str, principal: &str, op: i8) -> Acl {
    acl(TOPIC, name, LITERAL, principal, op, ALLOW)
}

pub(super) fn deny(name: &str, principal: &str, op: i8) -> Acl {
    acl(TOPIC, name, LITERAL, principal, op, DENY)
}

pub(super) fn rules(default_allow: bool, supers: &[&str]) -> Rules {
    Rules {
        default_allow,
        super_users: supers.iter().map(|s| (*s).to_owned()).collect(),
    }
}

pub(super) fn can(
    r: &Rules,
    set: &[Acl],
    (who, host): (&str, &str),
    (kind, name): (i8, &str),
    op: i8,
) -> bool {
    let set = AclSet::new(set.iter().cloned());
    r.authorize(
        &set,
        Subject {
            principal: who,
            host,
        },
        Resource { kind, name },
        op,
    )
}

pub(super) fn topic_can(r: &Rules, set: &[Acl], who: &str, name: &str, op: i8) -> bool {
    can(r, set, (who, "10.0.0.1"), (TOPIC, name), op)
}

#[test]
fn a_covering_allow_permits_and_nothing_else_does() {
    let r = rules(false, &[]);
    let set = [allow("t", "User:a", READ)];
    assert!(topic_can(&r, &set, "User:a", "t", READ));
    assert!(!topic_can(&r, &set, "User:a", "t", WRITE)); // SAD:511-514, the operation must be equal
    assert!(!topic_can(&r, &set, "User:b", "t", READ)); // another principal
    assert!(!topic_can(&r, &set, "User:a", "u", READ)); // another name: nothing covers it, the default (deny)
}

#[test]
fn deny_wins_over_allow_whatever_the_order() {
    let r = rules(true, &[]);
    for set in [
        [allow("t", "User:a", READ), deny("t", "User:a", READ)],
        [deny("t", "User:a", READ), allow("t", "User:a", READ)],
    ] {
        assert!(!topic_can(&r, &set, "User:a", "t", READ)); // SAD:584-587, the deny rule is built first
    }
    // A DENY of a wildcard beats a specific ALLOW (the wildcard section is scanned after, but still denies).
    let set = [allow("t", "User:a", READ), deny("*", "User:a", READ)];
    assert!(!topic_can(&r, &set, "User:a", "t", READ));
    // ... and a DENY of a prefix does too.
    let set = [
        allow("topic", "User:a", READ),
        acl(TOPIC, "top", PREFIXED, "User:a", READ, DENY),
    ];
    assert!(!topic_can(&r, &set, "User:a", "topic", READ));
}

#[test]
fn a_deny_is_exact_and_an_allow_implies() {
    let r = rules(false, &[]);
    // ALL matches every operation, for ALLOW and for DENY (SAD:495).
    let set = [allow("t", "User:a", ALL)];
    for op in [
        READ,
        WRITE,
        CREATE,
        DELETE,
        ALTER,
        DESCRIBE,
        DESCRIBE_CONFIGS,
        IDEMPOTENT_WRITE,
    ] {
        assert!(
            topic_can(&r, &set, "User:a", "t", op),
            "ALLOW ALL grants {op}"
        );
    }
    let set = [allow("t", "User:a", ALL), deny("t", "User:a", ALL)];
    assert!(!topic_can(&r, &set, "User:a", "t", READ));
    // READ, WRITE, DELETE, ALTER and DESCRIBE grant DESCRIBE (SAD:436-437, 505-507); CREATE does not.
    for op in [READ, WRITE, DELETE, ALTER, DESCRIBE] {
        let set = [allow("t", "User:a", op)];
        assert!(
            topic_can(&r, &set, "User:a", "t", DESCRIBE),
            "{op} implies describe"
        );
    }
    assert!(!topic_can(
        &r,
        &[allow("t", "User:a", CREATE)],
        "User:a",
        "t",
        DESCRIBE
    ));
    // ... but DESCRIBE grants nothing else, and the implication does not run the other way.
    assert!(!topic_can(
        &r,
        &[allow("t", "User:a", DESCRIBE)],
        "User:a",
        "t",
        READ
    ));
    // ALTER_CONFIGS grants DESCRIBE_CONFIGS and not the reverse (SAD:442-443, 508-509).
    assert!(topic_can(
        &r,
        &[allow("t", "User:a", ALTER_CONFIGS)],
        "User:a",
        "t",
        DESCRIBE_CONFIGS
    ));
    assert!(!topic_can(
        &r,
        &[allow("t", "User:a", DESCRIBE_CONFIGS)],
        "User:a",
        "t",
        ALTER_CONFIGS
    ));
    assert!(!topic_can(
        &r,
        &[allow("t", "User:a", ALTER_CONFIGS)],
        "User:a",
        "t",
        DESCRIBE
    ));
    // A DENY is exact (SAD:517): DENY READ does not deny DESCRIBE, and the ALLOW of READ still describes.
    let set = [allow("t", "User:a", READ), deny("t", "User:a", READ)];
    assert!(!topic_can(&r, &set, "User:a", "t", READ));
    assert!(topic_can(&r, &set, "User:a", "t", DESCRIBE)); // DENY READ does not touch DESCRIBE: the ALLOW READ implies it
    let set = [allow("t", "User:a", DESCRIBE), deny("t", "User:a", READ)];
    assert!(topic_can(&r, &set, "User:a", "t", DESCRIBE));
    let set = [allow("t", "User:a", ALL), deny("t", "User:a", WRITE)];
    assert!(topic_can(&r, &set, "User:a", "t", DESCRIBE));
    assert!(!topic_can(&r, &set, "User:a", "t", WRITE));
}

#[test]
fn the_default_applies_only_to_a_resource_no_acl_covers() {
    let allow_default = rules(true, &[]);
    let deny_default = rules(false, &[]);
    // Nothing at all: the default decides (SAD:113-114).
    assert!(topic_can(&allow_default, &[], "User:a", "t", READ));
    assert!(!topic_can(&deny_default, &[], "User:a", "t", READ));
    // An ACL for somebody else on the topic turns the default off for it, even when it is allowing (SAD:422).
    let set = [allow("t", "User:b", READ)];
    assert!(!topic_can(&allow_default, &set, "User:a", "t", READ));
    // ... and an ACL of another operation, or from another host, does the same.
    let set = [allow("t", "User:a", WRITE)];
    assert!(!topic_can(&allow_default, &set, "User:a", "t", READ));
    let mut elsewhere = allow("t", "User:a", READ);
    elsewhere.host = "9.9.9.9".into();
    assert!(!topic_can(
        &allow_default,
        &[elsewhere],
        "User:a",
        "t",
        READ
    ));
    // A different topic is untouched.
    assert!(topic_can(&allow_default, &set, "User:a", "other", READ));
    // A wildcard ACL covers every topic, and a prefix ACL the topics it starts.
    let set = [allow("*", "User:b", READ)];
    assert!(!topic_can(&allow_default, &set, "User:a", "other", READ));
    let set = [acl(TOPIC, "pre", PREFIXED, "User:b", READ, ALLOW)];
    assert!(!topic_can(&allow_default, &set, "User:a", "prefix", READ));
    assert!(topic_can(&allow_default, &set, "User:a", "other", READ));
    // The types are separate namespaces: a topic ACL says nothing of a group of that name.
    let set = [allow("t", "User:b", READ)];
    assert!(can(
        &allow_default,
        &set,
        ("User:a", "h"),
        (GROUP, "t"),
        READ
    ));
}

#[test]
fn a_super_user_bypasses_every_acl_including_a_deny() {
    let r = rules(false, &["User:admin"]);
    let set = [deny("t", "User:admin", ALL), deny("*", "User:*", ALL)];
    assert!(topic_can(&r, &set, "User:admin", "t", READ)); // SAD:236-238
    assert!(topic_can(&r, &[], "User:admin", "anything", ALTER));
    assert!(!topic_can(&r, &set, "User:other", "t", READ));
    // The match is the whole principal string.
    assert!(!topic_can(&r, &[], "User:Admin", "t", READ));
    assert!(!topic_can(&r, &[], "Group:admin", "t", READ));
}

#[test]
fn the_wildcard_principal_is_exactly_user_star() {
    let r = rules(false, &[]);
    let set = [allow("t", "User:*", READ)];
    assert!(topic_can(&r, &set, "User:anyone", "t", READ)); // SAD:463-469
    assert!(topic_can(&r, &set, "Group:g", "t", READ)); // any type
    let set = [allow("t", "Group:*", READ)];
    assert!(!topic_can(&r, &set, "Group:g", "t", READ));
    assert!(!topic_can(&r, &set, "User:anyone", "t", READ));
    // A DENY for User:* refuses everybody who is not a super user.
    let set = [allow("t", "User:a", READ), deny("t", "User:*", READ)];
    assert!(!topic_can(&r, &set, "User:a", "t", READ));
}
