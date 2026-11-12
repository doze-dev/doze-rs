//! Creation, filters and their words (kafka-semantics 1.7; the recorded `acl-unknown-codes`).

use super::*;
use crate::authz::operation::{ANY as OP_ANY, TWO_PHASE_COMMIT};
use crate::authz::pattern::{ANY as PAT_ANY, MATCH};
use crate::authz::permission::ANY as PERM_ANY;
use crate::authz::resource::{ANY as RES_ANY, USER};

/// The `good` creation of the recording: alice READ on the topic `acl`.
fn good() -> Acl {
    acl(TOPIC, "acl", LITERAL, "User:alice", READ, ALLOW)
}

fn with(f: impl FnOnce(&mut Acl)) -> Acl {
    let mut a = good();
    f(&mut a);
    a
}

#[test]
fn a_code_of_zero_is_found_in_each_field() {
    assert!(!good().has_zero_code());
    assert!(with(|a| a.resource_type = 0).has_zero_code());
    assert!(with(|a| a.pattern_type = 0).has_zero_code());
    assert!(with(|a| a.operation = 0).has_zero_code());
    assert!(with(|a| a.permission = 0).has_zero_code());
    // Only 0 counts: 99 and -1 are not UNKNOWN to the reader of the request.
    assert!(!with(|a| a.resource_type = 99).has_zero_code());
    assert!(!with(|a| a.operation = -1).has_zero_code());
}

#[test]
fn any_and_match_cannot_be_created_at_all() {
    // create-resource-any, create-pattern-any, create-pattern-match, create-operation-any, create-permission-any
    assert!(with(|a| a.resource_type = RES_ANY).has_filter_only_code());
    assert!(with(|a| a.pattern_type = PAT_ANY).has_filter_only_code());
    assert!(with(|a| a.pattern_type = MATCH).has_filter_only_code());
    assert!(with(|a| a.operation = OP_ANY).has_filter_only_code());
    assert!(with(|a| a.permission = PERM_ANY).has_filter_only_code());
    assert!(!good().has_filter_only_code());
    // A number that is no code is not ANY: it is refused later, by name.
    assert!(!with(|a| a.resource_type = 99).has_filter_only_code());
}

#[test]
fn a_creation_is_refused_in_kafkas_words_and_order() {
    assert_eq!(good().refusal(), None);
    // create-resource-99 and its three neighbours.
    let cases = [
        (
            with(|a| a.resource_type = 99),
            "Invalid resourceType UNKNOWN",
        ),
        (with(|a| a.pattern_type = 99), "Invalid patternType UNKNOWN"),
        (with(|a| a.operation = 99), "Invalid operation UNKNOWN"),
        (
            with(|a| a.permission = 99),
            "Invalid permissionType UNKNOWN",
        ),
        (
            with(|a| a.resource_type = -1),
            "Invalid resourceType UNKNOWN",
        ),
        (
            with(|a| a.name = String::new()),
            "Invalid empty resource name",
        ),
        (
            with(|a| a.principal = "alice".into()),
            "Could not parse principal from `alice` (no colon is present separating the principal type from the principal name)",
        ),
        (
            with(|a| (a.resource_type, a.name) = (CLUSTER, "other".into())),
            "The only valid name for the CLUSTER resource is kafka-cluster",
        ),
    ];
    for (a, want) in cases {
        assert_eq!(a.refusal().as_deref(), Some(want), "{a:?}");
    }
    // The handler's checks come before the controller's: an empty name is reported ahead of a bad code.
    let both = with(|a| (a.name, a.operation) = (String::new(), 99));
    assert_eq!(
        both.refusal().as_deref(),
        Some("Invalid empty resource name")
    );
    let both = with(|a| (a.resource_type, a.name, a.operation) = (CLUSTER, "x".into(), 99));
    assert_eq!(
        both.refusal().as_deref(),
        Some("The only valid name for the CLUSTER resource is kafka-cluster")
    );
}

#[test]
fn nothing_else_is_validated() {
    // A host may be anything, including empty; a principal `User:` or `User:*` passes; so does a name with any
    // character; PREFIXED `*` is allowed; an operation that is meaningless for the type is stored (ACM:135-177).
    for a in [
        with(|a| a.host = String::new()),
        with(|a| a.principal = "User:".into()),
        with(|a| a.principal = "User:*".into()),
        with(|a| a.principal = ":x".into()),
        with(|a| a.name = "we\0ird name".into()),
        with(|a| (a.pattern_type, a.name) = (PREFIXED, "*".into())),
        with(|a| a.operation = IDEMPOTENT_WRITE),
        with(|a| a.operation = TWO_PHASE_COMMIT),
        with(|a| a.resource_type = USER),
        with(|a| (a.resource_type, a.name) = (CLUSTER, CLUSTER_NAME.into())),
    ] {
        assert_eq!(a.refusal(), None, "{a:?}");
    }
}

fn filter(res: i8, pat: i8, op: i8, perm: i8) -> AclFilter {
    AclFilter {
        resource_type: res,
        pattern_type: pat,
        operation: op,
        permission: perm,
        ..AclFilter::any()
    }
}

#[test]
fn a_filter_is_checked_in_two_halves() {
    // The recorded DeleteAcls rows: any, topic-literal valid; 99 and -1 in the first two are the pattern filter,
    // in the last two the entry filter.
    assert_eq!(filter(1, 1, 1, 1).validate(), Ok(()));
    assert_eq!(filter(2, 3, 1, 1).validate(), Ok(()));
    assert_eq!(filter(99, 3, 1, 1).validate(), Err(FilterError::Pattern));
    assert_eq!(filter(-1, 3, 1, 1).validate(), Err(FilterError::Pattern));
    assert_eq!(filter(2, 99, 1, 1).validate(), Err(FilterError::Pattern));
    assert_eq!(filter(2, 3, 99, 1).validate(), Err(FilterError::Entry));
    assert_eq!(filter(2, 3, 1, 99).validate(), Err(FilterError::Entry));
    // The pattern half is judged first.
    assert_eq!(filter(99, 3, 99, 99).validate(), Err(FilterError::Pattern));
    assert_eq!(FilterError::Pattern.to_string(), "Unknown patternFilter.");
    assert_eq!(FilterError::Entry.to_string(), "Unknown entryFilter.");
    // The edges of each enumeration.
    assert_eq!(filter(7, 4, 15, 3).validate(), Ok(()));
    assert_eq!(filter(8, 4, 15, 3).validate(), Err(FilterError::Pattern));
    assert_eq!(filter(7, 5, 15, 3).validate(), Err(FilterError::Pattern));
    assert_eq!(filter(7, 4, 16, 3).validate(), Err(FilterError::Entry));
    assert_eq!(filter(7, 4, 15, 4).validate(), Err(FilterError::Entry));
    // A zero is a different matter: it closes the connection before any of this.
    assert!(filter(0, 3, 1, 1).has_zero_code());
    assert!(filter(2, 3, 1, 0).has_zero_code());
    assert!(!filter(99, 3, 1, 1).has_zero_code());
}

fn stored() -> Vec<Acl> {
    vec![
        allow("orders", "User:alice", READ),
        acl(TOPIC, "ord", PREFIXED, "User:bob", WRITE, DENY),
        allow("*", "User:alice", DESCRIBE),
        acl(GROUP, "orders", LITERAL, "User:alice", READ, ALLOW),
    ]
}

fn selected(f: &AclFilter) -> usize {
    stored().iter().filter(|a| f.matches(a)).count()
}

#[test]
fn the_any_filter_matches_everything_and_nulls_are_any() {
    assert_eq!(selected(&AclFilter::any()), 4);
    let mut f = AclFilter::any();
    f.resource_type = TOPIC;
    assert_eq!(selected(&f), 3);
    f.principal = Some("User:alice".into());
    assert_eq!(selected(&f), 2);
    f.operation = READ;
    assert_eq!(selected(&f), 1);
    f.permission = DENY;
    assert_eq!(selected(&f), 0);
}

#[test]
fn a_name_filter_is_exact_unless_the_pattern_is_match() {
    // ANY and the ACL's own pattern type compare names with equals (a PREFIXED filter does not do prefix matching).
    let mut f = AclFilter::any();
    f.name = Some("orders".into());
    assert_eq!(selected(&f), 2); // the topic and the group, both named `orders`
    f.pattern_type = PREFIXED;
    f.name = Some("ordersXYZ".into());
    assert_eq!(selected(&f), 0);
    f.name = Some("ord".into());
    assert_eq!(selected(&f), 1);
    f.pattern_type = LITERAL;
    f.name = Some("*".into());
    assert_eq!(selected(&f), 1);
    // MATCH: which ACLs apply to a resource of that name: the literal, the wildcard, and the covering prefixes.
    f.pattern_type = MATCH;
    f.name = Some("orders".into());
    f.resource_type = TOPIC;
    assert_eq!(selected(&f), 3);
    f.name = Some("ordering".into());
    assert_eq!(selected(&f), 2); // the prefix and the wildcard
    f.name = Some("zzz".into());
    assert_eq!(selected(&f), 1); // the wildcard
}

#[test]
fn a_host_filter_star_matches_only_acls_whose_host_is_star() {
    let mut other = allow("t", "User:a", READ);
    other.host = "10.0.0.1".into();
    let mut f = AclFilter::any();
    f.host = Some("*".into());
    assert!(!f.matches(&other));
    assert!(f.matches(&allow("t", "User:a", READ)));
    f.host = Some("10.0.0.1".into());
    assert!(f.matches(&other));
}

#[test]
fn the_set_selects_in_order_and_counts_a_duplicate_once() {
    let mut acls = stored();
    acls.extend(stored());
    let set = AclSet::new(acls);
    assert_eq!(set.len(), 4);
    assert_eq!(set.select(&AclFilter::any()).len(), 4);
    let mut sorted: Vec<_> = set.iter().cloned().collect();
    sorted.sort();
    assert_eq!(sorted, set.iter().cloned().collect::<Vec<_>>());
    assert!(AclSet::default().is_empty());
}

#[test]
fn a_principal_shape_has_a_type_and_a_name() {
    for good in ["User:admin", "User:a:b", "Group:g", "User:*"] {
        assert!(check_principal_shape(good), "{good}");
    }
    for bad in ["", "admin", "User:", ":admin", ":"] {
        assert!(!check_principal_shape(bad), "{bad:?}");
    }
}
