use std::collections::BTreeSet;

use super::*;

fn acl(kind: i8, name: &str, principal: &str, op: i8, perm: AclPermission) -> AclEntry {
    AclEntry {
        kind,
        name: name.to_owned(),
        pattern: AclPattern::Literal,
        principal: principal.to_owned(),
        host: "*".to_owned(),
        operation: op,
        permission: perm,
    }
}

fn text(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|s| match s {
            Span::Text(t) | Span::Mono(t) | Span::Strong(t) => t.as_str(),
        })
        .collect()
}

fn rules(default_allow: bool, supers: &[&str]) -> Rules {
    Rules {
        default_allow,
        super_users: supers
            .iter()
            .map(|s| (*s).to_owned())
            .collect::<BTreeSet<_>>(),
    }
}

#[test]
fn the_boards_delete_is_judged_as_the_board_says_it() {
    let orders_write = acl(
        resource::TOPIC,
        "orders",
        "User:orders",
        operation::WRITE,
        AclPermission::Allow,
    );
    let payments_read = acl(
        resource::TOPIC,
        "orders",
        "User:payments",
        operation::READ,
        AclPermission::Allow,
    );
    let all = [orders_write.clone(), payments_read];
    let said = text(&after_delete(&all, &orders_write, &rules(false, &[])));
    assert_eq!(
        said,
        "orders is also covered by 1 other ACL, so the default stays off for it and User:orders is then refused: its next Produce is answered TOPIC_AUTHORIZATION_FAILED (29)."
    );
}

#[test]
fn with_no_other_acl_the_default_decides() {
    let only = acl(
        resource::TOPIC,
        "orders",
        "User:orders",
        operation::WRITE,
        AclPermission::Allow,
    );
    let all = [only.clone()];
    let open = text(&after_delete(&all, &only, &rules(true, &[])));
    assert!(open.contains("covered by no ACL"), "{open}");
    assert!(
        open.contains("still allowed by the default (--acl-default allow)"),
        "{open}"
    );
    let shut = text(&after_delete(&all, &only, &rules(false, &[])));
    assert!(shut.contains("is then refused"), "{shut}");
}

#[test]
fn another_allow_for_the_same_principal_keeps_it_allowed() {
    let a = acl(
        resource::TOPIC,
        "orders",
        "User:orders",
        operation::WRITE,
        AclPermission::Allow,
    );
    let b = acl(
        resource::TOPIC,
        "orders",
        "User:*",
        operation::WRITE,
        AclPermission::Allow,
    );
    let said = text(&after_delete(&[a.clone(), b], &a, &rules(false, &[])));
    assert!(said.contains("still allowed by another ACL"), "{said}");
}

#[test]
fn deleting_a_deny_can_allow_again_and_a_super_user_never_changes() {
    let deny = acl(
        resource::TOPIC,
        "customer.profiles",
        "User:report-job",
        operation::READ,
        AclPermission::Deny,
    );
    let allow = acl(
        resource::TOPIC,
        "customer.profiles",
        "User:report-job",
        operation::READ,
        AclPermission::Allow,
    );
    let said = text(&after_delete(
        &[deny.clone(), allow],
        &deny,
        &rules(false, &[]),
    ));
    assert!(said.contains("is then allowed Read"), "{said}");
    let said = text(&after_delete(
        std::slice::from_ref(&deny),
        &deny,
        &rules(false, &["User:report-job"]),
    ));
    assert!(said.contains("is a super user"), "{said}");
}

#[test]
fn a_prefixed_acl_is_judged_at_its_own_prefix() {
    let mut p = acl(
        resource::TOPIC,
        "payments.",
        "User:payments",
        operation::WRITE,
        AclPermission::Allow,
    );
    p.pattern = AclPattern::Prefixed;
    let said = text(&after_delete(&[p.clone()], &p, &rules(false, &[])));
    assert!(said.contains("User:payments is then refused"), "{said}");
}

#[test]
fn the_error_a_refusal_is_met_as_follows_the_resource_and_the_operation() {
    let (api, err) = refused_as(resource::GROUP, operation::READ);
    assert_eq!((api, err.code()), (Some("JoinGroup"), 30));
    let (api, err) = refused_as(resource::TRANSACTIONAL_ID, operation::WRITE);
    assert_eq!((api, err.code()), (Some("InitProducerId"), 53));
    let (api, err) = refused_as(resource::CLUSTER, operation::IDEMPOTENT_WRITE);
    assert_eq!((api, err.code()), (Some("InitProducerId"), 31));
    let (api, err) = refused_as(resource::TOPIC, operation::WRITE);
    assert_eq!((api, err.code()), (Some("Produce"), 29));
    assert_eq!(
        refused_as(resource::TOPIC, operation::CLUSTER_ACTION).0,
        None
    );
}

#[test]
fn every_number_a_form_can_name_has_one_word_and_back() {
    for (word, label, wire, n) in KINDS {
        assert_eq!(kind_from_form(word), Some(n));
        assert_eq!(
            (kind_form(n), kind_label(n), kind_wire(n)),
            (word, label.to_owned(), wire.to_owned())
        );
    }
    for (word, label, wire, n) in OPERATIONS {
        assert_eq!(operation_from_form(word), Some(n));
        assert_eq!(
            (operation_form(n), operation_label(n), operation_wire(n)),
            (word, label.to_owned(), wire.to_owned())
        );
    }
    assert_eq!(kind_from_form("topic\n"), None);
    assert_eq!(operation_from_form(""), None);
    assert_eq!(kind_label(99), "Type 99");
    assert_eq!(operation_wire(99), "UNKNOWN_99");
    // The pattern and permission words are exactly two each; anything else reads as the first.
    assert_eq!(AclPattern::from_form("prefixed"), AclPattern::Prefixed);
    assert_eq!(AclPattern::from_form("x"), AclPattern::Literal);
    assert_eq!(AclPermission::from_form("deny"), AclPermission::Deny);
    assert_eq!(AclPermission::from_form("DENY"), AclPermission::Allow);
}

#[test]
fn the_operations_listed_are_exactly_the_ones_the_broker_stores() {
    let numbers: BTreeSet<i8> = OPERATIONS.iter().map(|o| o.3).collect();
    let stored: BTreeSet<i8> = (operation::ALL..=operation::TWO_PHASE_COMMIT).collect();
    assert_eq!(numbers, stored);
}

#[test]
fn an_entry_carries_its_own_address_to_the_delete_dialog() {
    let e = acl(
        resource::TOPIC,
        "a b&c",
        "User:x y",
        operation::IDEMPOTENT_WRITE,
        AclPermission::Deny,
    );
    let q = e.query();
    assert!(q.contains("type=topic"), "{q}");
    assert!(
        q.contains("name=a%20b%26c") || q.contains("name=a+b%26c"),
        "{q}"
    );
    assert!(
        q.contains("op=idempotent-write") && q.contains("perm=deny"),
        "{q}"
    );
}
