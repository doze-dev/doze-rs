//! The three ACL APIs, part two: what each stores, lists and removes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_wire::StrBytes;
use support::acls::{
    ALLOW, DENY, DESCRIBE, GROUP, LITERAL, PREFIXED, READ, TOPIC, WRITE, admin, closed_world,
    creation, filter,
};
use support::rig_with;

fn text(s: &str) -> Option<StrBytes> {
    Some(StrBytes::from_string(s.to_owned()))
}

/// An ACL as DescribeAcls lists it: (resource type, name, pattern), (principal, host), operation, permission.
fn entry(
    (res, name, pat): (i8, &str, i8),
    (who, host): (&str, &str),
    op: i8,
    perm: i8,
) -> support::acls::Entry {
    (
        res,
        name.to_owned(),
        pat,
        who.to_owned(),
        host.to_owned(),
        op,
        perm,
    )
}

#[tokio::test]
async fn acls_are_listed_by_resource_and_come_back_as_sent() {
    let rig = rig_with(closed_world);
    // The fixture of `sasl-plain`, and then some.
    let mut with_host = creation((TOPIC, "for-bob", LITERAL), "User:bob", DESCRIBE, ALLOW);
    with_host.host = StrBytes::from_string("10.0.0.7".to_owned());
    let alice = || creation((TOPIC, "for-alice", LITERAL), "User:alice", DESCRIBE, ALLOW);
    let bob = || creation((TOPIC, "for-bob", LITERAL), "User:bob", DESCRIBE, ALLOW);
    let group = creation((GROUP, "g", PREFIXED), "User:*", READ, DENY);
    // The same ACL twice, and once more in another request: stored once, success each time.
    let r = rig
        .create_acls_as(&admin(), 3, vec![bob(), alice(), group, with_host, alice()])
        .await;
    assert!(r.iter().all(|x| x.error_code == 0));
    assert_eq!(
        rig.create_acls_as(&admin(), 3, vec![alice()]).await[0].error_code,
        0
    );
    assert_eq!(
        rig.acls_as(&admin()).await,
        [
            entry(
                (TOPIC, "for-alice", LITERAL),
                ("User:alice", "*"),
                DESCRIBE,
                ALLOW
            ),
            entry(
                (TOPIC, "for-bob", LITERAL),
                ("User:bob", "*"),
                DESCRIBE,
                ALLOW
            ),
            entry(
                (TOPIC, "for-bob", LITERAL),
                ("User:bob", "10.0.0.7"),
                DESCRIBE,
                ALLOW
            ),
            entry((GROUP, "g", PREFIXED), ("User:*", "*"), READ, DENY),
        ]
    );
    // Grouped by resource: bob's two ACLs on `for-bob` are one resource.
    let all = rig.describe_acls_as(&admin(), 3, &filter(1, 1, 1, 1)).await;
    let sizes: Vec<_> = all.resources.iter().map(|r| r.acls.len()).collect();
    assert_eq!(sizes, [1, 2, 1]);
    // Every version that is served answers.
    for v in 1..=3 {
        let all = rig.describe_acls_as(&admin(), v, &filter(1, 1, 1, 1)).await;
        assert_eq!(all.resources.len(), 3, "v{v}");
    }
}

#[tokio::test]
async fn a_filter_selects_by_each_field_and_match_means_which_apply() {
    let rig = rig_with(closed_world);
    rig.create_acls_as(
        &admin(),
        3,
        vec![
            creation((TOPIC, "orders", LITERAL), "User:alice", READ, ALLOW),
            creation((TOPIC, "ord", PREFIXED), "User:bob", WRITE, DENY),
            creation((TOPIC, "*", LITERAL), "User:alice", DESCRIBE, ALLOW),
            creation((GROUP, "orders", LITERAL), "User:alice", READ, ALLOW),
        ],
    )
    .await;
    let count = |f: kafka_wire::messages::delete_acls_request::DeleteAclsFilter| {
        let rig = &rig;
        async move {
            rig.describe_acls_as(&admin(), 3, &f)
                .await
                .resources
                .iter()
                .map(|r| r.acls.len())
                .sum::<usize>()
        }
    };
    let mut f = filter(TOPIC, 1, 1, 1);
    assert_eq!(count(f.clone()).await, 3);
    f.principal_filter = text("User:alice");
    assert_eq!(count(f.clone()).await, 2);
    f.operation = READ;
    assert_eq!(count(f.clone()).await, 1);
    // MATCH on a name: the literal, the wildcard and the covering prefix.
    let mut m = filter(TOPIC, 2, 1, 1);
    m.resource_name_filter = text("orders");
    assert_eq!(count(m.clone()).await, 3);
    m.resource_name_filter = text("ordering");
    assert_eq!(count(m).await, 2);
    // A PREFIXED filter names the prefix exactly.
    let mut p = filter(1, PREFIXED, 1, 1);
    p.resource_name_filter = text("ordersXYZ");
    assert_eq!(count(p).await, 0);
}

#[tokio::test]
async fn delete_lists_what_each_filter_matched_and_removes_it_once() {
    let rig = rig_with(closed_world);
    rig.create_acls_as(
        &admin(),
        3,
        vec![
            creation((TOPIC, "a", LITERAL), "User:alice", READ, ALLOW),
            creation((TOPIC, "b", LITERAL), "User:alice", READ, ALLOW),
            creation((TOPIC, "c", LITERAL), "User:bob", READ, ALLOW),
        ],
    )
    .await;
    let mut alice = filter(TOPIC, 1, 1, 1);
    alice.principal_filter = text("User:alice");
    let mut a = filter(TOPIC, 1, 1, 1);
    a.resource_name_filter = text("a");
    let mut none_there = filter(GROUP, 1, 1, 1);
    none_there.resource_name_filter = text("nope");
    let r = rig
        .delete_acls_as(&admin(), 3, vec![alice, a, none_there, filter(99, 3, 1, 1)])
        .await;
    // Each filter is judged against the ACLs as they were before the request.
    assert_eq!(
        r.iter().map(|x| x.matching_acls.len()).collect::<Vec<_>>(),
        [2, 1, 0, 0]
    );
    assert_eq!(
        r.iter()
            .map(|x| (x.error_code, x.error_message.is_some()))
            .collect::<Vec<_>>(),
        [(0, false), (0, false), (0, false), (42, true)]
    );
    let m = &r[1].matching_acls[0];
    assert_eq!(
        (
            m.error_code,
            m.resource_type,
            m.resource_name.to_string(),
            m.pattern_type
        ),
        (0, TOPIC, "a".to_owned(), LITERAL)
    );
    assert_eq!(
        (
            m.principal.to_string(),
            m.host.to_string(),
            m.operation,
            m.permission_type
        ),
        ("User:alice".to_owned(), "*".to_owned(), READ, ALLOW)
    );
    assert_eq!(rig.acls_as(&admin()).await.len(), 1);
    // The empty request of `version-matrix`: answered with an empty list.
    assert!(rig.delete_acls_as(&admin(), 3, vec![]).await.is_empty());
    assert!(rig.create_acls_as(&admin(), 3, vec![]).await.is_empty());
}
